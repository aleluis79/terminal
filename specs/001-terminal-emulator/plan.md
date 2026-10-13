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
