# Plan 001 — Terminal Emulator

## 1. Estructura de módulos

```
terminal/
├── Cargo.toml
├── src/
│   ├── main.rs              # Entry point, inicia config + app
│   │
│   ├── config/              # RF-31 a RF-43
│   │   ├── mod.rs           # Estructura Config + defaults
│   │   ├── loader.rs        # RF-31, RF-32, RF-34: lee TOML, genera default
│   │   ├── cli.rs           # RF-33: parsea argumentos CLI
│   │   ├── error.rs         # RF-35: errores de config
│   │   └── validate.rs      # CL-3: validación de valores
│   │
│   ├── core/                # RF-11 a RF-24, RF-25, RF-26 (sin dependencias GUI)
│   │   ├── mod.rs
│   │   ├── grid.rs          # Grid 2D de celdas (CharacterCell + attributes)
│   │   ├── buffer.rs        # RF-15: scrollback, active screen, alternate screen
│   │   ├── parser.rs        # RF-11, RF-12, RF-13: parser ECMA-48 / ANSI
│   │   ├── state.rs         # Estado del terminal: cursor, modes, attributes
│   │   ├── input.rs         # RF-26: keyboard → bytes al PTY
│   │   └── hyperlink.rs     # RF-18: OSC-8 link tracking
│   │
│   ├── pty/                 # RF-25, RF-27, RF-28, CL-1, CL-2
│   │   ├── mod.rs           # Trait PTY (abstracción cross-platform)
│   │   ├── unix.rs          # Linux: nix crate, fork + pts
│   │   └── win32.rs         # Windows: pty crate, ConPTY
│   │
│   ├── render/              # RF-11 a RF-24, RF-36, RF-37, RF-39, RF-40, RF-41, RF-42
│   │   ├── mod.rs           # Pipeline: grid → GPU
│   │   ├── font.rs          # RF-36, RF-37, RF-19, RF-20, CL-5: font loading
│   │   ├── glyph.rs         # swash: shaping + glyph rendering
│   │   ├── atlas.rs         # Glyph atlas en GPU (texture atlas)
│   │   ├── color.rs         # RF-12: Color conversion (16/256/RGB)
│   │   ├── renderer.rs      # Draw calls: text, cursor, selection, background
│   │   └── image.rs         # RF-21: Kitty graphics protocol
│   │
│   ├── gui/                 # RF-1 a RF-10, RF-29, RF-30, RF-38, RF-43
│   │   ├── mod.rs           # Application lifecycle
│   │   ├── window.rs        # RF-1, RF-38, RF-39, RF-40: ventana + resize
│   │   ├── tab_bar.rs       # RF-4, RF-5: barra de pestañas
│   │   ├── tabs.rs          # RF-2, RF-3: gestión de pestañas
│   │   ├── split.rs         # RF-6 a RF-10: división y navegación de paneles
│   │   ├── event.rs         # RF-29: mouse events, text selection
│   │   ├── clipboard.rs     # RF-29, RF-30: clipboard con modifiers
│   │   ├── keymap.rs        # RF-43: keyboard bindings
│   │   └── platform/        # Platform-specific glue
│   │       ├── mod.rs
│   │       ├── x11.rs       # Linux: X11 connection
│   │       └── win.rs       # Windows: Win32 API
│   │
│   └── session/             # RF-25, RF-27, RF-28, CL-1, CL-2
│       ├── mod.rs           # TerminalSession: PTY + parser + buffer
│       ├── process.rs       # RF-25, CL-1: spawn shell, read/write PTY
│       └── signals.rs       # RF-27, RF-28: Ctrl+C, Ctrl+D → SIGINT/EOF
```

### Dependencias entre crates

```
config  ──→ (independiente)
core    ──→ (independiente)
pty     ──→ core (usa PTY, pasa datos al parser)
render  ──→ core (lee grid, usa wgpu/fontdb/swash)
gui     ──→ core, config, render, pty, session
session ──→ core, pty
```

### Responsabilidades de cada módulo

| Módulo | Responsabilidad | RFs cubiertos |
|--------|----------------|---------------|
| `config` | Cargar TOML, validar, defaults | RF-31 a RF-43 |
| `core::grid` | Grid 2D de celdas con atributos | RF-11 a RF-15 |
| `core::parser` | Parser de secuencias ANSI/ECMA-48 | RF-11 a RF-18, RF-22 a RF-24 |
| `core::state` | Cursor, modes, attributes | RF-13, RF-16 |
| `core::input` | Keyboard → bytes PTY | RF-26 |
| `pty` | Abstracción PTY cross-platform | RF-25, RF-27, RF-28 |
| `render` | GPU rendering pipeline | RF-11 a RF-24, RF-36, RF-37, RF-39 a RF-42 |
| `gui::window` | Creación y gestión de ventana | RF-1, RF-38 a RF-40 |
| `gui::tab_bar` | Barra de pestañas | RF-4, RF-5 |
| `gui::tabs` | Crear/cerrar/switch pestañas | RF-2, RF-3 |
| `gui::split` | Dividir/navegar paneles | RF-6 a RF-10 |
| `gui::event` | Mouse selection | RF-29 |
| `gui::clipboard` | Clipboard con modifiers | RF-29, RF-30 |
| `gui::keymap` | Keyboard bindings | RF-43 |
| `session` | PTY + parser + buffer unidos | RF-25, RF-27, RF-28, CL-1 a CL-2 |

## 2. Modelo de datos

### Entidades principales

```rust
// Configuración
struct Config {
    window: WindowConfig,
    font: FontConfig,
    cursor: CursorConfig,
    colors: ColorScheme,
    keyboard: KeyboardConfig,
}

// Ventana
struct Window {
    title: String,
    dimensions: Dimensions,  // columns × rows
    opacity: f32,
    tabs: Vec<Tab>,
    active_tab_idx: usize,
    winit_window: Option<winit::window::Window>,
}

// Pestaña
struct Tab {
    title: String,
    panels: Vec<Panel>,
    active_panel_idx: usize,
    layout: SplitLayout,     // árbol binario de divisiones
}

// Panel
struct Panel {
    session: TerminalSession,
    bounds: Rect,             // posición y tamaño en la pestaña
    selection: Option<Selection>,
}

// Sesión de terminal
struct TerminalSession {
    pty: Box<dyn Pty>,
    parser: Parser,
    buffer: GridBuffer,       // active + scrollback
    state: TerminalState,
}

// Grid (pantalla)
struct GridBuffer {
    active: Grid,             // pantalla visible (rows × cols)
    scrollback: VecDeque<Grid>, // historial (máx 10000 líneas)
}

struct Grid {
    rows: usize,
    cols: usize,
    cells: Vec<Cell>,         // array plano para cache locality
}

struct Cell {
    glyph: Glyph,             // character codepoint
    style: CellStyle,         // fg, bg, bold, italic, underline, etc.
}
```

### Relaciones
- `Application` → muchos `Window` (actualmente 1)
- `Window` → muchos `Tab` → muchos `Panel`
- `Panel` → uno `TerminalSession` → uno `Pty`
- `TerminalSession` → uno `GridBuffer` (active + scrollback)

### Persistencia
- Configuración: `$XDG_CONFIG_HOME/terminal/config.toml` (Linux) / `%APPDATA%\terminal\config.toml` (Windows)
- Sin base de datos ni historial persistente entre sesiones (fuera de alcance).

## 3. Decisiones justificadas

### 3.1. Framework de ventanas: winit
| Alternativa | Por qué se eligió winit | Por qué se descartó |
|-------------|------------------------|---------------------|
| **winit** ✅ | Cross-platform (Linux X11 + Windows), ligero, usado por Alacritty/WezTerm, buen soporte de eventos | — |
| egui | Es una librería de UI inmediata, no tiene windowing nativo bajo demanda | Demasiado acoplado a una UI framework |
| gtk4-rs | Nativo en Linux pero difícil en Windows | No cross-platform limpio |
| iced | Bonito pero orientado a apps, no a terminal emulador | Menos control bajo pedido |

### 3.2. Renderizado: wgpu (software o GPU)
| Alternativa | Por qué se eligió wgpu | Por qué se descartó |
|-------------|------------------------|---------------------|
| **wgpu** ✅ | WebGPU backend en Rust, soporta CPU y GPU, usado por Alacritty | — |
| OpenGL | Deprecado en macOS, difícil en Windows moderno | Tecnología obsoleta |
| Softbuffer | Software-only, simple pero limitado | No escala a features como ligatures/emoji bien |
| skia | Grande, complejo, dependencias pesadas | Stack pesado |

### 3.3. PTY: abstracción propia con crates platform-specific
| Alternativa | Por qué se eligió | Por qué se descartó |
|-------------|-------------------|---------------------|
| **Trait PTY + nix (Linux) / pty (Windows)** ✅ | Control total, testing fácil, sin acople | — |
| `shell-escape` + `Command` | Simple pero no maneja PTX real | No soporta terminal interactiva |
| `rust-pty` crate | Abstracción lista | Pocas dependencias, menos control |

### 3.4. Font rendering: fontdb + swash
| Alternativa | Por qué se eligió | Por qué se descartó |
|-------------|-------------------|---------------------|
| **fontdb + swash** ✅ | fontdb maneja fallback de fuentes, swash renderiza sin FreeType | — |
| font-kit + freetype-rs | Más complejo, font-kit desmantelado | Depende de C |
| ab_glyph | Simple pero menos features para ligatures | Menos maduro para terminal |

### 3.5. Serialización: serde + toml
- **TOML** elegido sobre YAML porque es más simple, tipado, y usado por Alacritty/WezTerm.
- **serde** es el estándar de facto en Rust, bien mantenido, license Apache-2.0.

### 3.6. Escalera de implementación (MVP primero)
Se implementa en orden de dependencia, con entregables incrementales:
1. **MVP:** ventana + terminal básico (sin tabs ni splits) → cubre RF-1, RF-11 a RF-15, RF-25 a RF-28
2. **Tabs:** RF-2, RF-3, RF-4, RF-5
3. **Splits:** RF-6 a RF-10
4. **Config:** RF-31 a RF-43
5. **Avanzado:** RF-16 a RF-24, RF-29, RF-30

## 4. Estrategia de tests

### 4.1. Tipos de test

| Tipo | Dónde | Qué cubre |
|------|-------|-----------|
| **Unit tests** | `core/`, `config/`, `pty` (mock) | Parser ANSI, grid operations, config parsing/validation |
| **Integration tests** | `tests/` | Session completa (PTY mock → parser → buffer → output) |
| **Snapshot tests** | `render/` | Renderizado de caracteres, colores, ligatures |
| **Manual tests** | Demo checklist | Flujo completo: tabs, splits, config, clipboard |

### 4.2. Cobertura por requisito

| RF range | Tipo de test | Cobertura |
|----------|-------------|-----------|
| RF-11 a RF-15 | Unit + Integration | Parser ECMA-48, colores, estilos, alternate screen, scrollback |
| RF-16 a RF-24 | Unit + Integration | Cursor shapes, bracketed paste, OSC-8, bell, window events |
| RF-25 a RF-28 | Integration (PTY mock) | Read/write PTY, Ctrl+C, Ctrl+D |
| RF-29, RF-30 | Manual | Clipboard con modifiers (no automatizable fácilmente) |
| RF-31 a RF-35 | Unit | Config parsing, defaults, errors, CLI overrides |
| RF-36 a RF-43 | Unit + Manual | Font, window, cursor, colors, keymap config |
| RF-1 a RF-5 | Manual | Window, tabs |
| RF-6 a RF-10 | Manual | Panel splits |

### 4.3. Tests específicos por caso límite

| CL | Test |
|----|------|
| CL-1 | `session_test_pty_exits` — mock PTY envía EOF → session cierra gracefully |
| CL-2 | `parser_test_invalid_utf8` — bytes inválidos → U+FFFD, no crash |
| CL-3 | `config_test_invalid_values` — font inexistente/tamaño 0 → defaults |
| CL-4 | `window_test_rapid_resize` — múltiples resize events → sin parpadeo |
| CL-5 | `font_test_glyph_fallback` — glyph no soportado → next font |
| CL-6 | `tab_test_rapid_create` — 10 tabs en 1s → todos creados |
| CL-7 | `tab_test_last_close` — última pestaña cerrada → app cierra |
| CL-8 | `buffer_test_flood_write` — 1M chars → scrollback intacto |

### 4.4. CI
- `cargo test` en GitHub Actions: Linux (Ubuntu) + Windows (latest)
- `cargo clippy -- -D warnings`
- `cargo fmt --check`

## 5. Extensión: Compatibilidad con aplicaciones TUI (RF-44 a RF-51)

Esta sección amplía las anteriores y refleja la estructura real del workspace por
crates (`terminal_core`, `terminal_render`, `terminal_gui`, `terminal`).

### 5.1. Estructura de módulos afectados

```
terminal_core/src/
├── cell.rs       # RF-44, RF-45: Cell gana el rol de ancho (cabecera/continuación)
├── state.rs      # RF-44, RF-48, RF-49, RF-50: cursor visible/forma, SavedCursor, modo sincronizado
├── parser.rs     # RF-47 a RF-51, CL-13: nuevas secuencias y consultas (ver 5.2)
├── engine.rs     # RF-44, RF-45, CL-9, CL-10, CL-11: escritura/erasure con ancho
├── buffer.rs     # RF-45: limpieza de parejas anchas, reflow y scrollback
└── session.rs    # RF-47, RF-51: expone modo sincronizado y respuestas a consultas

terminal_render/src/
├── font.rs       # RF-46: cadena de fallback (familia → símbolos → emoji)
├── glyph.rs      # RF-44, RF-46: glifos anchos y ausencia de `.notdef`
└── renderer.rs   # RF-44, RF-45, RF-48: dibuja el glifo ancho una vez, salta la continuación, respeta cursor visible

terminal_gui/src/ + terminal/src/main.rs
├── window.rs     # RF-47: presentar solo al cerrar la sincronización (con timeout)
└── main.rs       # RF-47, RF-48, RF-51: leer modos, responder consultas al PTY, ocultar cursor
```

| Módulo | Responsabilidad | RFs cubiertos |
|--------|-----------------|---------------|
| `core::cell` | Rol de ancho de cada celda (1/2/continuación) | RF-44, RF-45 |
| `core::engine` | Calcular ancho al escribir y limpiar hermanas | RF-44, RF-45, CL-9 a CL-11 |
| `core::parser` | Reconocer `?2026`, `?25`, DECSCUSR, SCP/RCP y consultas | RF-47 a RF-51, CL-13 |
| `core::state` | `cursor.visible`, forma/parpadeo, `SavedCursor`, modo sincronizado | RF-47 a RF-50 |
| `core::session` | Difundir modo sincronizado y componer respuestas | RF-47, RF-51 |
| `render::font` | Cadena de fallback de fuentes | RF-46, CL-14 |
| `render::glyph`/`renderer` | Render de glifos anchos y sin solapes | RF-44 a RF-46 |
| `gui`/`terminal` | Presentar con sincronización, cursor visible, respuestas al PTY | RF-47, RF-48, RF-51 |

### 5.2. Modelo de datos

```rust
// terminal_core/src/cell.rs — RF-44, RF-45
struct Cell {
    c: char,
    style: CellStyle,
    width: u8, // 2 = cabecera de ancho doble, 1 = normal, 0 = celda hermana/continuación
}

// terminal_core/src/state.rs — RF-44, RF-48 a RF-50
struct Cursor {
    row: usize,
    col: usize,
    shape: CursorShape,
    visible: bool,   // DECTCEM (?25)
    blink: bool,     // DECSCUSR 1/3/5 = parpadeante, 2/4/6 = fijo
}

struct SavedCursor {                 // ESC[s / ESC[u
    row: usize,
    col: usize,
    shape: CursorShape,
    attributes: TextAttributes,
    fg: Color,
    bg: Color,
}

struct TerminalModes {
    // …existentes…
    synchronized_output: bool,       // ?2026
}

// terminal_core/src/parser.rs — RF-47 a RF-51
enum Action {
    // …existentes…
    SetCursorVisible(bool),          // RF-48
    SetSynchronizedOutput(bool),     // RF-47
    SaveCursor,                      // RF-50
    RestoreCursor,                   // RF-50
    SetCursorBlink(bool),            // RF-49
    QueryCursorPosition,             // RF-51 (DSR, ESC[6n)
    QueryDeviceAttributes,           // RF-51 (DA, ESC[c)
}
```

Persistencia: sin cambios (la spec no añade estado persistente).

### 5.3. Decisiones justificadas

| Tema | Decisión | Alternativa descartada | Razón |
|------|----------|------------------------|-------|
| Ancho de carácter | crate `unicode-width` (MIT/Apache) en `terminal_core` | tabla propia; calcular en el renderer | Constitución 1 (stack mínimo y licencia) y 3 (core sin GUI); una tabla propia se desactualiza con Unicode |
| Marcado de celdas anchas | `Cell.width` (0/1/2) con cabecera + continuación | guardar un `String`/grapheme por celda; duplicar el carácter en ambas celdas | Cambio mínimo: el atlas y la caché siguen indexados por `char`; duplicar el carácter haría que el renderer dibujase dos veces el glifo |
| Combinantes | se ignoran si no hay carácter base | fusionar en un grapheme complejo | CL-11; el clustering completo (modo 2027) queda fuera de alcance por coste/beneficio |
| Emoji | cadena de fallback explícita con las fuentes del sistema | incrustar una fuente de emoji como asset | Evita assets pesados (Constitución 1); CL-14 cubre el caso sin fuente |
| Sincronizado `?2026` | flag en el buffer; el event loop no presenta entre `h` y `l`, con timeout | ignorar la secuencia | CL-12 exige no dejar la pantalla congelada si el `l` no llega |
| Respuestas a consultas | el parser emite `Action::Query*` y el GUI compone/escribe la respuesta al PTY | responder dentro del parser/core | Constitución 3: el core no conoce tamaño en píxeles ni el color del tema |
| DECSCUSR | `ESC[Ps q` (intermedio espacio) → forma + parpadeo | depender solo de OSC 22 | Es lo que emiten los TUIs reales (opencode envía `ESC[1 q`) |
| Guardar/restaurar | `SavedCursor` en `TerminalState` | no soportar SCP/RCP | `ESC[s`/`ESC[u` se usan al medir el layout |

### 5.4. Estrategia de tests

| Unidad de requisito | Tipo | Casos representativos |
|---------------------|------|-----------------------|
| RF-44 | Unit (`core`) | ancho de `a`=1, `日`=2, combinante=0; el cursor avanza 1/2/0 |
| RF-44 / CL-9 | Unit (`core`) | ancho doble en la última columna → envuelve completo, sin partirlo |
| RF-45 / CL-10 | Unit (`core`) | escribir angosto sobre la celda hermana borra el carácter ancho; ECH/EL/IL/DL limpian parejas |
| CL-11 | Unit (`core`) | combinante sin base → se ignora y el cursor no avanza |
| RF-46 / CL-14 | Unit (`render`) | glifo ausente en la familia → símbolos → emoji; `.notdef` solo como último recurso |
| RF-47 / CL-12 | Unit (`core`) + Manual | `?2026h`/`l` conmutan el modo; el timeout del GUI presenta sin `l` |
| RF-48 | Unit (`core`) | `?25l`/`?25h` conmutan `cursor.visible` |
| RF-49 | Unit (`core`) | `ESC[0..6 q` → forma y parpadeo correctos |
| RF-50 | Unit (`core`) | `ESC[s` … `ESC[u` restauran posición y forma |
| RF-51 | Unit (`core`) + Integration | DSR responde `ESC[f;cR`; DA responde identificación; OSC 10/11 `?` generan query |
| CL-13 | Unit (`core`) | `ESC[>…m` y `ESC[>…q` no se interpretan como SGR/otro (ya cubierto) |

**Verificación end-to-end (manual, no en CI):** regrabar una sesión real de
`opencode` a través de un PTY, reinyectarla en `terminal_core` y renderizarla en
una textura *offscreen* (ejemplos tipo `render_capture`/`dump_grid`), comparando
rejilla y píxeles contra una referencia (`pyte`). Este procedimiento fue el que
localizó CL-13 y sirve para validar RF-44 a RF-51 sin depender de un entorno
gráfico.

### 5.5. Divisor de paneles (RF-52)

Hoy el hueco entre paneles (`SPLIT_GAP`) queda del color del fondo, sin línea.
Se añade un divisor visible sin cambiar la geometría del reparto:

| Módulo | Responsabilidad | RF |
|--------|-----------------|----|
| `gui::split` | `divider_rects(area, gap)`: rectángulos de los huecos entre hermanos | RF-52 |
| `render::renderer` | `RenderContent.dividers` + `divider_color`; dibuja cada hueco como un quad sólido | RF-52 |
| `terminal` | Calcula los divisores de la pestaña activa y los pasa al renderer | RF-52 |

Decisión: dibujar el divisor como una banda **sólida de 1 px centrada en el
hueco**, no rellenar todo el hueco. Alternativas descartadas: rellenar el hueco
completo (engorda la separación y mueve el área de cada panel) o dibujar bordes
de panel (dependería del color de fondo del programa). Color: tomado del tema
(color de acento/cursor), sin nuevo parámetro de configuración.

Tests: Unit (`gui::split`) de `divider_rects` (1 split → 1 rectángulo en el
hueco correcto; 2 splits anidados → 2 rectángulos) y verificación visual con
`render_capture` sobre una captura con paneles divididos.

## 6. Desplazamiento vertical del scrollback (RF-53, RF-54)

Hoy el scrollback existe pero nunca se alimenta (solo se guardaba la pantalla
entera al entrar en la pantalla alterna) y el render siempre dibuja la rejilla
activa. Se pasa a un historial por líneas con un desplazamiento de vista.

### 6.1. Estructura de módulos

| Módulo | Responsabilidad | RF |
|--------|-----------------|----|
| `core::buffer` | Historial por líneas (`VecDeque<Vec<Cell>>`), población al hacer scroll, offset de vista y `visible_rows` | RF-15, RF-53, RF-54 |
| `core::engine` | Vuelve al final del buffer al escribir nueva salida | RF-54 |
| `gui::keymap` | Nuevas `GuiAction` de scroll y bindings por defecto (Shift+…) | RF-53 |
| `terminal` | Rueda del ratón y despacho de las `GuiAction` de scroll al panel activo | RF-53 |

### 6.2. Modelo de datos

```rust
struct GridBuffer {
    // …existente…
    scrollback: VecDeque<Vec<Cell>>, // una línea (fila) por entrada
    scroll_offset: usize,            // líneas desplazadas hacia el historial
}

impl GridBuffer {
    fn push_scrollback_line(&mut self);      // copia la fila 0 que sale
    pub fn scroll_back(&mut self, n: usize); // limitado por scrollback.len()
    pub fn scroll_forward(&mut self, n: usize);
    pub fn scroll_to_bottom(&mut self);
    pub fn scroll_to_top(&mut self);
    pub fn visible_rows(&self) -> Vec<Vec<Cell>>; // historial + rejilla activa
}
```

Notas de decisión: se descarta guardar la pantalla entera por entrada
(desperdicio y semántica incorrecta); el historial es por línea, como en
xterm. `visible_rows` devuelve `offset` líneas del final del historial seguidas
de las primeras `rows - offset` de la rejilla activa.

### 6.3. Decisiones justificadas

| Tema | Decisión | Alternativa descartada | Razón |
|------|----------|------------------------|-------|
| Grano del historial | una línea (`Vec<Cell>`) por entrada | `Grid` completo por entrada | Coste de memoria y semántica de líneas de xterm; el código actual guardaba pantallas enteras |
| Dónde vive el offset | `GridBuffer` (por panel) | estado global del GUI | Cada panel tiene su propio historial y desplazamiento |
| Reset | nueva salida (`write_char`) vuelve al final | mantener la vista hasta que el usuario baje | Es el comportamiento esperado de una terminal |
| Pantalla alterna | sin historial (offset 0) | reutilizar el historial del main | La pantalla alterna no tiene scrollback (CL-16) |
| Entrada | rueda + Shift+PageUp/PageDown/Home/End | solo rueda | Cubre teclado sin depender del protocolo de ratón del programa |

### 6.4. Estrategia de tests

| RF / CL | Tipo | Casos |
|---------|------|-------|
| RF-15, RF-53 | Unit (`core`) | `main_line_feed` llena el historial; el límite se respeta |
| RF-53 / CL-15 | Unit (`core`) | `scroll_back` se detiene en la línea más antigua |
| RF-53 | Unit (`core`) | `visible_rows` mezcla historial y rejilla activa en el orden correcto |
| RF-54 / CL-16 | Unit (`core`) | escribir nueva salida resetea el offset; la pantalla alterna ignora el scroll |
| RF-53 | Unit (`gui`) | los bindings Shift+PageUp/PageDown/Home/End resuelven a las nuevas acciones |
| RF-53 | Manual | rueda del ratón en la app con salida larga (`seq 1 200`) |

## 7. Control de scroll y pantalla alterna (RF-55, RF-56)

Origen: `glow -p` no usa su propio paginador, ejecuta `less -r`. Para
desplazarse **hacia arriba**, `less` posiciona el cursor en la última fila y
emite `ESC M` (Índice Inverso / RI) repetidamente. El parser ignoraba `ESC M`,
así que subir en `less`/`glow` no movía la pantalla (la salida se comparó contra
`pyte` y difería). Además, la rueda del ratón no llegaba a la aplicación en
pantalla alterna, por lo que tampoco se podía paginar con el ratón.

### 7.1. Estructura de módulos

| Módulo | Responsabilidad | RF |
|--------|-----------------|----|
| `core::parser` | Reconocer `ESC D` (IND), `ESC M` (RI) y `ESC E` (NEL) como acciones propias | RF-55 |
| `core::engine` | Aplicar IND/RI/NEL: mover cursor o desplazar la pantalla según la fila | RF-55, CL-17, CL-18 |
| `core::buffer` | `scroll_down(n)` real (desplaza el contenido hacia abajo y blanquea la parte superior) y reutilizar `main_line_feed`/`alternate_line_feed` para IND | RF-55, CL-17, CL-18 |
| `terminal` | Rueda del ratón: si la pantalla alterna está activa, reenviar flechas a la aplicación | RF-56 |

### 7.2. Modelo de datos

```rust
// terminal_core/src/parser.rs — RF-55
enum Action {
    // …existentes…
    Index,        // ESC D
    ReverseIndex, // ESC M
    NextLine,     // ESC E
}

// terminal_core/src/buffer.rs — RF-55
impl ScreenBuffer {
    // Sustituye el stub actual (no-op) por un desplazamiento real.
    pub fn scroll_down(&mut self, n: usize);
}
```

Sin estado nuevo: IND/RI/NEL reutilizan el cursor y el buffer existentes; el
desplazamiento hacia arriba en la pantalla principal sigue pasando por
`push_scrollback_line` (RF-15, CL-18).

### 7.3. Decisiones justificadas

| Tema | Decisión | Alternativa descartada | Razón |
|------|----------|------------------------|-------|
| Alcance de RI | `ESC M` mueve el cursor si no está en la fila 0; en la fila 0 desplaza el contenido hacia abajo | desplazar siempre el contenido | Es la semántica VT100 y lo que `less` espera al paginar |
| Historial en RI | no alimenta el scrollback | insertar la línea superior en el historial | RI inserta contenido *nuevo* arriba; el historial solo registra lo que sale por arriba (CL-17) |
| IND en la pantalla principal | reutiliza `main_line_feed` (la línea que sale entra al historial) | no tocar el historial | CL-18; comportamiento consistente con LF |
| NEL | IND + columna 0 | acción separada con lógica duplicada | NEL es IND con retorno de carro (ECMA-48) |
| Origen del scroll en alterna | reenviar flechas a la aplicación | mover el offset del scrollback | La pantalla alterna no tiene historial (CL-16); el paginador es quien decide cuánto avanzar |
| Codificación de las flechas | reutilizar `input::key_event_to_bytes` | bytes `ESC[A` a mano | Una sola fuente de verdad si se añade DECCKM más adelante |
| Mouse reporting | fuera de alcance (CL-19) | implementar `?1000/1002/1003/1006` | La spec lo excluye explícitamente ("Fuera de alcance") |

### 7.4. Estrategia de tests

| RF / CL | Tipo | Casos |
|---------|------|-------|
| RF-55 | Unit (`core`) | `ESC D`/`ESC M`/`ESC E` se parsean a `Index`/`ReverseIndex`/`NextLine` |
| RF-55 | Unit (`core`) | RI en fila > 0 solo sube el cursor; IND en fila < última solo baja el cursor; NEL baja y va a columna 0 |
| RF-55 / CL-17 | Unit (`core`) | RI en la fila 0 inserta una línea en blanco arriba y descarta la última, sin tocar el scrollback |
| RF-55 / CL-18 | Unit (`core`) | IND en la última fila de la pantalla principal mete la línea que sale en el scrollback; en alterna la descarta |
| RF-55 | Unit (`core`) | `ScreenBuffer::scroll_down(n)` desplaza y blanquea las `n` filas superiores |
| RF-56 | Unit (`gui`) | con pantalla alterna activa, la rueda produce bytes de flecha; sin ella, produce scroll de scrollback |
| RF-55 | Manual/end-to-end | capturar `less`/`glow -p` con un PTY (bajar y subir) y comparar la rejilla con `pyte` |

## 8. Región de scroll DECSTBM (RF-57)

Origen: el modo TUI de `glow` (bubbletea) no usa `less`: define una región de
scroll con `ESC[Pt;Pbr` (p. ej. filas 1..29, dejando la última para la barra de
estado) y pagina moviendo el cursor al margen inferior y emitiendo `LF`, o
insertando líneas (`ESC[L`) en el margen superior. El motor ignoraba la región,
así que `LF`/`IL` desplazaban **toda** la pantalla (incluida la barra de estado)
y el contenido quedaba desfasado.

### 8.1. Estructura de módulos

| Módulo | Responsabilidad | RF |
|--------|-----------------|----|
| `core::parser` | Reconocer `ESC[Pt;Pbr` → `Action::SetScrollRegion(top, bottom)` | RF-57 |
| `core::engine` | Validar la región, aplicarla y situar el cursor en el margen superior | RF-57, CL-22 |
| `core::buffer` | `scroll_top`/`scroll_bottom` en `ScreenBuffer`; `scroll_up`/`scroll_down`, `main_line_feed`/`alternate_line_feed`, `insert_lines`/`delete_lines` y RI acotados a la región | RF-57, CL-20, CL-21 |

### 8.2. Modelo de datos

```rust
// terminal_core/src/buffer.rs — RF-57
struct ScreenBuffer {
    // …existentes…
    scroll_top: usize,    // primera fila de la región (0-indexed)
    scroll_bottom: usize, // última fila de la región (inclusive)
}
impl ScreenBuffer {
    pub fn set_scroll_region(&mut self, top: usize, bottom: usize);
}
```

`scroll_top`/`scroll_bottom` por defecto abarcan toda la pantalla, así que sin
`DECSTBM` el comportamiento es el actual. `resize` reajusta la región (la
extiende si abarcaba toda la pantalla; la recorta en otro caso).

### 8.3. Decisiones justificadas

| Tema | Decisión | Alternativa descartada | Razón |
|------|----------|------------------------|-------|
| Modelo de la región | dos índices en `ScreenBuffer` (por buffer) | estado global del motor | La pantalla principal y la alterna tienen márgenes independientes |
| Cursor al definir la región | se mueve al margen superior (VT100) | dejarlo donde estaba (como `pyte`) | Es el comportamiento de xterm/VTE; las apps (bubbletea) igualmente posicionan el cursor justo después, así que no altera la verificación end-to-end |
| Scrollback | solo si la región abarca toda la pantalla | siempre que la fila 0 salga | Un scroll dentro de una subregión no representa salida real de la pantalla (CL-20) |
| `IL`/`DL` fuera de la región | se ignoran | aplicarlos igualmente | xterm los descarta si el cursor está fuera de los márgenes |
| `LF` por debajo del margen | el cursor se acota al margen inferior | bajarlo hasta el final de la pantalla | Comportamiento de `pyte`/xterm; además bubbletea siempre se posiciona en el margen |
| `SU`/`SD` (`ESC[S`/`ESC[T`) | fuera de alcance de esta tarea | implementarlos | bubbletea no los usa; se mantiene el cambio enfocado |

### 8.4. Estrategia de tests

| RF / CL | Tipo | Casos |
|---------|------|-------|
| RF-57 | Unit (`core`) | `ESC[2;4r` → `SetScrollRegion(2,4)`; `ESC[;29r` → `SetScrollRegion(1,29)` |
| RF-57 / CL-20 | Unit (`core`) | con región 2..4, `LF` en el margen inferior desplaza solo la región y no toca filas 1 y 5; no alimenta el scrollback |
| RF-57 / CL-21 | Unit (`core`) | con región 2..4, `RI` en el margen superior desplaza la región hacia abajo |
| RF-57 | Unit (`core`) | `IL`/`DL` dentro de la región mueven las filas de la región; fuera de ella no hacen nada |
| RF-57 / CL-22 | Unit (`core`) | `ESC[4;2r` (inválida) se ignora |
| RF-57 | Unit (`core`) | `resize` reajusta la región |
| RF-57 | Manual/end-to-end | capturar el TUI de `glow` (`glow -t AGENTS.md`) bajando con flechas y comparar con `pyte` |

## 9. Color de la barra de pestañas (RF-58)

La barra se pintaba con la paleta ANSI (`color(0)` para el fondo y las
pestañas inactivas, `color(8)` para la activa). Con temas donde `ansi.black` es
claro (p. ej. Catppuccin, `#45475a`) la barra queda gris y contrasta con el
fondo de la terminal. Pasa a usar el **fondo del tema**
(`palette.background`, de `colors.primary.background`).

| Módulo | Responsabilidad | RF |
|--------|-----------------|----|
| `render::renderer` | `push_tab_bar`: fondo de la barra y pestañas inactivas con `palette.background`; pestaña activa como chip oscuro (`color(0)`) con esquinas redondeadas vía SDF; dibujo de la `×` | RF-58, RF-59 |
| `gui::tab_bar` | `TabRect` reserva el área de la `×` (`close_x`) y expone `close_hit` | RF-59 |
| `terminal` | Clic en la barra: la `×` cierra la pestaña, el resto la enfoca | RF-59 |

Decisiones:

- No se añaden claves de configuración nuevas; el fondo del tema ya es la
  referencia del *chrome* (RF-42) y la pestaña activa usa el slot ANSI "negro".
- Esquinas redondeadas: se calcula la distancia con signo en el *fragment
  shader* a partir de las coordenadas locales del quad y el radio (nuevos
  atributos `local`/`params` en `QuadVertex`). Alternativas descartadas:
  geometría de esquinas (más vértices y triángulos por pestaña) o un pipeline
  aparte (más código y un *draw call* extra). El radio es 0 para el resto de
  quads, así que el coste es un par de operaciones por fragmento.
- El chip activo se separa verticalmente del borde de la barra para que el
  redondeo no toque el borde superior.
- La `×` se dibuja en todas las pestañas (una celda), centrada en el área
  reservada a la derecha del título.

Tests: unitarios de `gui::tab_bar` (`layout` reserva la `×`, `close_hit`
distingue el borde) y verificación manual con una vista previa offscreen (la
elección de colores y el redondeo necesitan GPU, no entran en CI).

## 10. Identidad e icono de la aplicación (RF-60)

La ventana solo fijaba el *título* (`with_title`), así que Wayland no tenía
`app_id` (el shell mostraba "desconocido") y, sin `.desktop`, tampoco icono.

| Módulo | Responsabilidad | RF |
|--------|-----------------|----|
| `terminal/src/main.rs` | `WindowAttributes::with_name("terminal", "terminal")` en Linux (trait `WindowAttributesExtWayland`, que escribe el `app_id`/`WM_CLASS` compartido) | RF-60 |
| `assets/terminal.desktop` | Entrada de escritorio (`Icon=terminal`, `StartupWMClass=terminal`) | RF-60 |
| `assets/terminal.png` | Icono 256×256 | RF-60 |
| `assets/install-desktop.sh` | Instala la entrada y el icono en `$XDG_DATA_HOME` | RF-60 |

Decisiones:

- En Wayland el icono de ventana no existe como protocolo: el shell lo toma de
  la entrada `.desktop` cuyo nombre coincide con el `app_id`. Por eso el
  `app_id` debe ser `terminal` y el archivo `terminal.desktop`.
- No se añade el icono vía `with_window_icon`: requeriría embeber/decodificar
  una imagen (dependencia extra) y no tiene efecto en Wayland. En GNOME/X11 el
  `.desktop` (vía `WM_CLASS`) también resuelve el icono.
- El `Exec` del `.desktop` es `terminal`, pensado para una instalación del
  binario en el `PATH`; el lanzamiento desde el repositorio (`cargo run`) ya se
  asocia por `app_id`.

Tests: verificación manual: `WAYLAND_DEBUG=1` muestra
`xdg_toplevel.set_app_id("terminal")`; `desktop-file-validate` acepta la
entrada.

## 11. Selección de shell (RF-62 a RF-67)

Origen: el binario lanzaba siempre `$SHELL` (Unix) o `%COMSPEC%` (Windows), sin
forma de elegir otra shell. Se añade configuración (`shell.program`/`shell.args`
y una lista de perfiles) más atajos para abrir una pestaña con un perfil
concreto, con *fallback* a la shell por defecto si el programa falla.

### 11.1. Estructura de módulos

| Módulo | Responsabilidad | RF |
|--------|-----------------|----|
| `terminal_config::config` | `ShellConfig` (`shell.program`, `shell.args`, `shell.profiles[]`) dentro de `Config` | RF-62, RF-64 |
| `terminal_config::shell` | Resolución pura: perfiles efectivos, comando por defecto/perfil N y `program_exists` (búsqueda en `PATH`) | RF-64, RF-65, CL-26, CL-29 |
| `terminal_config::cli` | `--shell` sobrescribe `shell.program` | RF-63 |
| `terminal_pty::pty`/`unix`/`win32` | `spawn(program, args)`: pasar argumentos al proceso (argv real en Unix; *command line* citada en ConPTY) | RF-62, RF-64, CL-25 |
| `terminal_core::session` | `spawn_command(program, args)`; `spawn_shell` delega con argumentos vacíos | RF-62 |
| `terminal_gui::keymap` | `GuiAction::NewTabWithProfile(usize)` + bindings por defecto Ctrl+Shift+1…9 | RF-66 |
| `terminal/src/main.rs` | Resuelve el comando (perfil por defecto, perfil N del atajo), lanza la sesión y aplica el *fallback* visible | RF-62 a RF-67, CL-24, CL-27 |

### 11.2. Modelo de datos

```rust
// terminal_config/src/config.rs — RF-62, RF-64
pub struct ShellConfig {
    pub program: Option<String>,      // shell por defecto (None = shell del sistema)
    pub args: Vec<String>,
    pub profiles: Vec<ShellProfile>,  // perfiles elegibles
}
pub struct ShellProfile {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}

// terminal_config/src/shell.rs — RF-64, RF-65
pub struct ShellCommand {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}
impl Config {
    pub fn shell_profiles(&self) -> Vec<ShellProfile>;    // configurados, o por defecto
    pub fn default_shell_command(&self) -> ShellCommand;  // program/args, o perfil 0
    pub fn profile_command(&self, index: usize) -> Option<ShellCommand>;
}
pub fn program_exists(program: &str) -> bool;             // ruta explícita o PATH (+PATHEXT)

// terminal_gui/src/keymap.rs — RF-66
pub enum GuiAction { /* …existentes… */ NewTabWithProfile(usize) }
struct PhysicalBinding { code: KeyCode, modifiers: u64, action: GuiAction }
```

Reglas de resolución: `shell.profiles` no vacío ⇒ esos perfiles (se descartan
los que no tengan `name`/`program`, CL-26); vacío ⇒ perfiles por defecto
(Windows: `cmd`, `powershell`, `wsl` de los que existan; otros: `$SHELL`).
`shell.program` tiene prioridad como shell del perfil por defecto (RF-62). El
perfil N del atajo es `shell_profiles()[N]`; si no existe, no se abre pestaña
(CL-27).

Persistencia: la misma `config.toml` (secciones `[shell]` y
`[[shell.profiles]]`); sin estado nuevo.

### 11.3. Decisiones justificadas

| Tema | Decisión | Alternativa descartada | Razón |
|------|----------|------------------------|-------|
| Dónde vive la resolución | `terminal_config::shell`, puro y sin GUI | en `terminal/src/main.rs` | Testeable sin GPU ni PTY (constitución 4); el binario queda fino |
| Modelo | lista de perfiles `name/program/args` (estilo Windows Terminal) | una única shell configurable | RF-64/RF-65 piden elegir entre cmd/powershell/wsl |
| Perfiles por defecto | autodetectados en Windows si `shell.profiles` está vacío | exigir configuración manual | Que funcione sin escribir config (RF-65) |
| Existencia de programa | búsqueda en `PATH` (con `PATHEXT` en Windows) | asumir que existe | RF-65/CL-29: ofrecer solo los que existen |
| Argumentos | `Vec<String>` con `argv` real | interpolar `"prog args"` en una sola cadena | Evita reinterpretación (CL-25) y problemas de *quoting* |
| Comando por defecto | `shell.program`+`args` si está; si no, perfil 0 | solo perfiles | RF-62 es independiente de la lista de perfiles |
| Atajo por perfil | Ctrl+Shift+1…9 resueltos por **código físico** (`KeyCode::DigitN`) | por `Key` lógico | Con Shift, `Key::Character` es el símbolo de la distribución (`!`, `"`, …) y el atajo se rompería; el código físico es independiente del *layout* |
| Precedencia de bindings | lógicos (config RF-43 + defaults) primero; físicos después | físicos primero | Los bindings de usuario siguen ganando |
| Sesión dividida | hereda la shell del panel dividido (RF-70) | usar el perfil por defecto | El usuario quiere conservar la shell al dividir; ver §13 |
| Fallback | `spawn` falla ⇒ línea de error visible en el panel + shell por defecto | dejar la pestaña sin sesión | RF-67 y CL-24 ("sin dejar la pestaña vacía") |

### 11.4. Estrategia de tests

| RF / CL | Tipo | Casos |
|---------|------|-------|
| RF-62 | Unit (`config`) | `[shell] program/args` se parsean y sobreviven el *round-trip* TOML |
| RF-63 | Unit (`config::cli`) | `--shell` sobrescribe `shell.program`; sin el flag no cambia |
| RF-64 | Unit (`config::shell`) | perfiles configurados ⇒ esos; `shell.profiles` vacío ⇒ por defecto; `profile_command(n)` |
| RF-65 | Unit (`config::shell`) | solo se ofrecen los programas existentes; sin perfiles, el por defecto es `$SHELL`/`%COMSPEC%` |
| CL-26 | Unit (`config::shell`) | perfil sin `name` o sin `program` se descarta |
| CL-29 | Unit (`config::shell`) | `program_exists("wsl.exe")` falso cuando no está; no se ofrece el perfil |
| RF-62/64, CL-25 | Unit (`pty`) | `spawn(program, args)` ejecuta con argumentos (Windows: `cmd /c echo`); args con espacios llegan como un argumento |
| RF-66 | Unit (`gui::keymap`) | `resolve_physical(Digit1, Ctrl+Shift)` ⇒ `NewTabWithProfile(0)`; el binding físico no depende del *layout* |
| RF-67, CL-24, CL-27 | Manual / Integration | atajo de un perfil inexistente no abre pestaña; `--shell programa-inexistente` muestra el error y abre la shell por defecto |
| RF-62 a RF-67 | Manual | `--shell wsl.exe`, `[[shell.profiles]]` con cmd/powershell/wsl y pestañas por atajo |

## 12. Codificación de teclas especiales (RF-68, RF-69)

### 12.1. Estructura de módulos

| Módulo | Cambio |
|--------|--------|
| `terminal_core/src/input.rs` | `KeyEvent::Special`: `Escape` devuelve `0x1b`; `Delete` construye `ESC[3~` (o `ESC[3;N~`) |
| `terminal_gui/src/event.rs` | Solo tests/comentarios: `key_to_terminal_event` ya traduce `NamedKey::{Escape, Delete}` |

### 12.2. Modelo de datos

Sin estructuras nuevas: el modelo sigue siendo `KeyEvent::Special { key, ctrl, alt }` y `key_event_to_bytes` es una función pura `KeyEvent -> Vec<u8>`.

| Tecla | Sin modificadores | Alt | Ctrl | Alt+Ctrl |
|-------|-------------------|-----|------|----------|
| Escape | `0x1b` | `0x1b` | `0x1b` | `0x1b` |
| Supr | `ESC[3~` | `ESC[3;3~` | `ESC[3;5~` | `ESC[3;7~` |

### 12.3. Decisiones justificadas

| Tema | Decisión | Alternativa descartada | Razón |
|------|----------|------------------------|-------|
| Escape | devolver el byte `ESC` y terminar | `ESC[[D` | RF-68: `ESC[[D` no es una secuencia estándar; `0x1b` es lo que envía xterm y lo que esperan readline/vim/nano |
| Supr | `ESC[3~`, con el `3` antes del modificador | reutilizar el prefijo `ESC[`+modificador (`ESC[;5~`) | La forma estándar es `CSI 3 ~` / `CSI 3 ; N ~`; reutilizar el prefijo no puede expresar el `3` intermedio |
| Código de modificador | `1 + alt*2 + ctrl*4` (alt=3, ctrl=5, ambos=7) | el orden de la rama de flechas | Es el orden xterm estándar y el que ya usa la rama `Special` para `Home`/`End` |
| Escape con modificadores | ignora `ctrl`/`alt` | `ESC`+modificador | Escape no tiene forma `CSI`; xterm envía `0x1b` siempre |
| Flechas | fuera de alcance | corregir también su orden de modificadores | La rama `KeyEvent::Arrow` intercambia alt/ctrl respecto al estándar; no lo cubre ningún RF (ver «Dudas abiertas») |

### 12.4. Estrategia de tests

| RF / CL | Tipo | Casos |
|---------|------|-------|
| RF-68 | Unit (`core::input`) | `Escape` ⇒ `[0x1b]`; con Alt y/o Ctrl también `[0x1b]` |
| RF-69 | Unit (`core::input`) | `Delete` ⇒ `ESC[3~`; con Alt ⇒ `ESC[3;3~`; con Ctrl ⇒ `ESC[3;5~`; con ambos ⇒ `ESC[3;7~` |
| CL-30 | Unit (`core::input`, `gui::event`) | no se genera `ESC[[D` ni `ESC[~`; el atajo del GUI produce `0x1b` |

## 13. Shell heredada al dividir paneles (RF-70)

### 13.1. Estructura de módulos

| Módulo | Cambio |
|--------|--------|
| `terminal/src/main.rs` | `TabState.panes` pasa a `HashMap<PanelId, PaneState>`; `spawn_session` devuelve `PaneState` (sesión + shell efectiva); `split_active` hereda la shell del panel enfocado |

No se toca `terminal_core` ni `terminal_config`: la herencia es política del binario (constitución 4, el core no sabe de paneles).

### 13.2. Modelo de datos

```rust
// terminal/src/main.rs
struct PaneState {
    session: TerminalSession,
    command: ShellCommand,  // shell que realmente corre (RF-70, CL-31)
}
struct TabState {
    layout: SplitLayout,
    panes: HashMap<PanelId, PaneState>,
}
impl TabState {
    fn split_command(&self, default: &ShellCommand) -> ShellCommand;
}
```

`spawn_session` guarda como `command` la petición si arrancó, o
`ShellCommand::platform_default()` si hubo *fallback* (RF-67), de modo que
`split_command` devuelve siempre la shell que corre (CL-31).

### 13.3. Decisiones justificadas

| Tema | Decisión | Alternativa descartada | Razón |
|------|----------|------------------------|-------|
| Dónde se guarda la shell | por panel (`PaneState.command`) | una sola shell por pestaña | RF-70 habla del "panel dividido"; deja la puerta abierta a paneles con distinta shell |
| Qué se hereda | la shell efectiva (ya con *fallback*) | la shell solicitada original | CL-31: el panel nuevo debe arrancar algo que funcione; heredar la solicitada repetiría el error |
| Forma del mapa | `PaneState { session, command }` | mapa paralelo `commands: HashMap<PanelId, …>` | Evita dos mapas que hay que mantener sincronizados |
| Pestaña nueva | sigue usando el perfil por defecto o el perfil N (RF-62…RF-66) | heredar de la pestaña activa | Una pestaña nueva no "divide"; RF-70 solo cubre los paneles |
| Sin panel enfocado | cae al perfil por defecto | no abrir el panel | El panel siempre existe al dividir; la rama es defensiva |

### 13.4. Estrategia de tests

| RF / CL | Tipo | Casos |
|---------|------|-------|
| RF-70 | Unit (`terminal` bin) | `TabState` cuyo panel corre la shell X ⇒ `split_command(&otra)` devuelve X, no `otra` |
| CL-31 | Unit (`terminal` bin) | panel con shell inexistente (acaba en la del sistema, RF-67) ⇒ `split_command` devuelve la del sistema, no la inexistente |
| RF-70 | Manual | abrir pestaña con un perfil (Ctrl+Shift+N) y dividir: el panel nuevo arranca la misma shell |







