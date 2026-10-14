# Tasks 001 — Terminal Emulator

## Fase 0: Infraestructura

- [x] **T001: Inicializar proyecto Rust con estructura de workspace**
  - RF: —
  - Crear `Cargo.toml` workspace con crates: `terminal_core`, `terminal_config`, `terminal_pty`, `terminal_render`, `terminal_gui`, `terminal` (binary)
  - Configurar `cargo clippy`, `cargo fmt`, dependencias de desarrollo (rstest, assert_cmd)
  - Hecho cuando: `cargo build` compila todo el workspace sin warnings

- [x] **T002: Configurar CI básica (GitHub Actions)**
  - RF: —
  - `.github/workflows/ci.yml`: `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` en Linux y Windows
  - Hecho cuando: CI verde en push de prueba

## Fase 1: Core del terminal (MVP)

- [x] **T003: Implementar estructura de Cell y Grid (array 2D plano)**
  - RF: RF-11, RF-15
  - `terminal_core/src/grid.rs`: Cell (char + fg_color + bg_color + attributes), Grid (rows × cols, array plano), get/set, clear
  - Hecho cuando: tests unitarios de get/set en esquinas y centro, clear funciona, grid resize mantiene contenido

- [x] **T004: Implementar TerminalState (cursor, modes, attributes)**
  - RF: RF-13, RF-16
  - `terminal_core/src/state.rs`: Cursor (position, shape), Mode (wraparound, insert, origin), Attributes (bold, italic, underline, inverse, hidden, strike)
  - Hecho cuando: tests de cambio de estado, cursor move, attribute toggle

- [x] **T005: Implementar Parser ANSI/ECMA-48 básico**
  - RF: RF-11, RF-12, RF-13
  - `terminal_core/src/parser.rs`: FSM parser que reconoce CSI, ESC, OSC prefix. CSI sequences: `m` (SGR), `H` (CUP), `J` (ED), `K` (EL), `l`/`h` (DECSET/DECRESET)
  - Hecho cuando: tests con rstest para cada secuencia CSI reconocida, bytes inválidos no crash (CL-2)

- [x] **T006: Implementar colores (16, 256, 24-bit)**
  - RF: RF-12
  - `terminal_core/src/color.rs`: Color enum (16, 256(u8), RGB(u8,u8,u8)), SGR color parsing (`m` con 5 o 8 params)
  - Hecho cuando: tests de parsing de cada modo de color, conversion a RGBA

- [x] **T007: Integrar Parser + Grid + State en TerminalEngine**
  - RF: RF-11, RF-12, RF-13, RF-15
  - `terminal_core/src/engine.rs`: TerminalEngine que recibe bytes → parser → actualiza Grid + State
  - Hecho cuando: input de bytes "Hello\x1b[31mWorld" → grid con "Hello" normal + "World" rojo

- [x] **T008: Implementar scrollback buffer**
  - RF: RF-15
  - `terminal_core/src/buffer.rs`: GridBuffer con active grid + scrollback (VecDeque de Grid, máx 10000 líneas), scroll up/down, preserve content on resize
  - Hecho cuando: escribir 10001 líneas → scrollback tiene 10000, active tiene rows actuales, resize no pierde scrollback (CL-8)

- [x] **T009: Implementar alternate screen**
  - RF: RF-14
  - `terminal_core/src/state.rs`: Mode alternate_screen, secuencia `?1049` (DECSET/DECRESET), dos buffers de pantalla
  - Hecho cuando: enviar `?1049h` → alternate screen activo, enviar `?1049l` → vuelve a screen normal con contenido preservado

- [x] **T010: Implementar bracketed paste mode**
  - RF: RF-17
  - `terminal_core/src/state.rs`: Mode bracketed_paste, secuencia `?2004h`/`?2004l`, parser reconoce OSC 200 (`^[[200~` ... `^[[201~`)
  - Hecho cuando: activar bracketed paste → paste se rodea con markers, desactivar → paste normal

- [x] **T011: Implementar OSC-8 hyperlinks**
  - RF: RF-18
  - `terminal_core/src/hyperlink.rs`: Hyperlink ID tracking, OSC 8 parsing (`]8;;URL^]8;;^\\`), cell tiene opcional hyperlink_id
  - Hecho cuando: enviar `]8;;https://example.com^\\Link^]8;;^\\` → celda con hyperlink asociado

- [x] **T012: Implementar bell (audible y visual)**
  - RF: RF-23
  - `terminal_core/src/state.rs`: Bell handler, BellAction enum (Audible, Visual), BEL character (0x07) y OSC bell
  - Hecho cuando: enviar BEL → bell triggered, enviar OSC bell → bell triggered, test que bell se cuenta

## Fase 2: PTY y Sessions

- [x] **T013: Definir trait PTY y implementar Unix (Linux)**
  - RF: RF-25, RF-27, RF-28
  - `terminal_pty/src/unix.rs`: Trait PTY con spawn(), read(), write(), pid(), wait(). Unix: fork + posix_openpt + grantpt + unlockpt + ptsname + exec shell
  - Hecho cuando: spawn bash → read devuelve datos, write envía datos, Ctrl+C → SIGINT al child (RF-27)

- [x] **T014: Implementar PTY para Windows (ConPTY)**
  - RF: RF-25, RF-27, RF-28
  - `terminal_pty/src/win32.rs`: Trait PTY con Windows ConPTY API (CreatePseudoConsole, CreateProcess)
  - Hecho cuando: spawn cmd.exe en Windows → read/write funciona, Ctrl+C → SIGINT

- [x] **T015: Implementar TerminalSession (PTY + Parser + Buffer)**
  - RF: RF-25, RF-26, RF-27, RF-28
  - `terminal_core/src/session.rs`: TerminalSession une PTY + Parser + GridBuffer. Thread de lectura PTY → parser → buffer. write() → PTY
  - Hecho cuando: spawn shell → datos aparecen en buffer, enviar teclas → llegan al shell, Ctrl+C → SIGINT, Ctrl+D → EOF, child exit → session notifica

- [x] **T016: Manejar child process exit (CL-1)**
  - RF: CL-1
  - `terminal_core/src/session.rs`: Detectar PTY EOF → session marca como exited → notifica a GUI → GUI muestra mensaje de salida
  - Hecho cuando: child process muere → session reporta exit, GUI muestra "Process exited" sin crash

## Fase 3: Renderizado

- [x] **T017: Configurar winit window + wgpu device**
  - RF: RF-1, RF-39
  - `terminal_gui/src/window.rs`: Winit event loop, wgpu instance + adapter + device + queue, surface para present
  - Hecho cuando: ventana se abre con winit, wgpu device inicializado, surface creada

- [x] **T018: Implementar font loading (fontdb)**
  - RF: RF-36, RF-37, RF-19, RF-20, CL-5
  - `terminal_render/src/font.rs`: fontdb database, load font family, fallback chain, CLS (Character Level Shift) para ligatures
  - Hecho cuando: cargar "JetBrains Mono" con tamaño configurable → fontdb tiene glyphs, fallback a monospace si no existe (CL-5)

- [x] **T019: Implementar glyph rendering (swash)**
  - RF: RF-11, RF-19, RF-20
  - `terminal_render/src/glyph.rs`: swash cache → shape text → get glyphs → draw to bitmap. Ligature support, emoji support
  - Hecho cuando: renderizar "Hello 🦀" → bitmap con todos los glyphs, ligatura "fi" → single glyph

- [x] **T020: Implementar glyph atlas (GPU texture)**
  - `terminal_render/src/atlas.rs`: Texture atlas en GPU, upload glyphs desde swash bitmaps, UV mapping para cada glyph
  - Hecho cuando: atlas crece dinámicamente, glyphs se mapean a UVs correctos, draw call usa atlas

- [x] **T021: Implementar renderer principal**
  - RF: RF-11 a RF-15, RF-36, RF-37, RF-39, RF-41, RF-42
  - `terminal_render/src/renderer.rs`: Pipeline: Grid → glyph atlas → draw calls. Background color, text, cursor (block/underline/beam), selection highlight
  - Hecho cuando: grid con texto + cursor → frame renderizado a pantalla, cursor cambia forma según config, selección se highlight

- [x] **T022: Implementar colores en renderer**
  - RF: RF-12, RF-42
  - `terminal_render/src/color.rs`: Color → GPU format (RGBA8Unorm), palette 256, RGB 24-bit, config colors (background/foreground)
  - Hecho cuando: grid con colores 16/256/RGB → renderizado correcto, config colors aplicada

- [x] **T023: Implementar resize sin parpadeo**
  - RF: RF-39, CL-4
  - `terminal_render/src/renderer.rs`: Resize surface → recreate render targets → re-render sin flicker
  - Hecho cuando: resize ventana 10 veces rápido → sin parpadeo, scrollback preservado (CL-4)

- [x] **T024: Implementar Kitty graphics protocol**
  - RF: RF-21
  - `terminal_render/src/image.rs`: OSC 6 record images, decode PNG/SVG/GIF, render inline en grid cells
  - Hecho cuando: enviar OSC 6 image → image renderizada en posición correcta

## Fase 4: Tabs

- [x] **T025: Implementar Tab y TabManager**
  - RF: RF-2, RF-3, RF-4, RF-5
  - `terminal_gui/src/tabs.rs`: Tab (title, panels, active_panel), TabManager (tabs, active_idx, add, remove, switch)
  - Hecho cuando: crear tab → añadido a lista, cerrar tab → adyacente enfocado, última tab → app cierra (CL-7)

- [x] **T026: Implementar TabBar UI**
  - RF: RF-4, RF-5
  - `terminal_gui/src/tab_bar.rs`: Renderizar tabs en la parte superior, click → switch tab, hover effect
  - Hecho cuando: múltiples tabs → barra visible, click en tab → switch efectivo

- [x] **T027: Bindings de teclado para tabs**
  - RF: RF-2, RF-3
  - `terminal_gui/src/keymap.rs`: Ctrl+Shift+N → new tab, Ctrl+Shift+W → close tab, Ctrl+Shift+Tab/Shift+Tab → switch tab
  - Hecho cuando: Ctrl+Shift+N → nueva tab, Ctrl+Shift+W → cierra tab activa

## Fase 5: Splits (Paneles)

- [x] **T028: Implementar SplitLayout (árbol binario)**
  - RF: RF-6, RF-7, RF-8, RF-9, RF-10
  - `terminal_gui/src/split.rs`: SplitNode enum (Leaf(Panel) | Split(Horizontal, Box<SplitNode>, Box<SplitNode>, f32 ratio)). split_h(), split_v(), close(), navigate()
  - Hecho cuando: split horizontal → 2 paneles, split vertical → 2 paneles, close → panel adyacente enfocado, navigate H/J/K/L → panel vecino

- [x] **T029: Implementar split dividers y resize con ratón**
  - RF: RF-9
  - `terminal_gui/src/split.rs`: Draw dividers, drag to resize ratio, drag detection (mouse down on divider)
  - Hecho cuando: arrastrar divider → paneles se redimensionan proporcionalmente

- [x] **T030: Bindings de teclado para splits**
  - RF: RF-6, RF-7, RF-8, RF-10
  - `terminal_gui/src/keymap.rs`: Ctrl+Shift+↓ → split horizontal, Ctrl+Shift+→ → split vertical, Ctrl+Shift+X → close panel, Ctrl+Shift+H/J/K/L → navigate
  - Hecho cuando: todos los bindings de splits funcionan

## Fase 6: Configuración

- [x] **T031: Definir estructura Config con serde**
  - RF: RF-31, RF-36 a RF-43
  - `terminal_config/src/lib.rs`: Structs Config, WindowConfig, FontConfig, CursorConfig, ColorScheme, KeyboardConfig con derives Serialize/Deserialize
  - Hecho cuando: serde deserialize ejemplo TOML completo sin errores

- [x] **T032: Implementar config loader (TOML file)**
  - RF: RF-31, RF-32, RF-34
  - `terminal_config/src/loader.rs`: find_config_path(), load_config() → Result<Config>, generate_default() → writes file
  - Hecho cuando: no existe config → generate_default crea archivo, existe config → load_config la lee y aplica

- [x] **T033: Implementar CLI argument parser**
  - RF: RF-33
  - `terminal_config/src/cli.rs`: clap parser, --config path, --font, --font-size, --geometry WxH. Merge con file config (CLI overrides)
  - Hecho cuando: --font "Fira Code" → config.font.family = "Fira Code", sin CLI → file config usado

- [x] **T034: Implementar config validation**
  - RF: RF-35, CL-3
  - `terminal_config/src/validate.rs`: validate_font(), validate_size(), validate_opacity(). Invalid → warn + use default
  - Hecho cuando: font inexistente → warn + default, tamaño 0 → warn + default, opacity 2.0 → warn + default, TOML inválido → error message + defaults

## Fase 7: Input y Clipboard

- [x] **T035: Implementar keyboard event handling**
  - RF: RF-26, RF-43
  - `terminal_gui/src/event.rs`: Winit KeyEvent → TerminalInput (key code + modifiers). Keymap resolution: config bindings → defaults
  - Hecho cuando: cualquier tecla → session recibe bytes, keymap custom funciona (RF-43)

- [x] **T036: Implementar text selection con ratón**
  - RF: RF-29
  - `terminal_gui/src/event.rs`: Mouse button down → start selection, mouse move → extend selection, mouse up → end selection. Selection bounds → text
  - Hecho cuando: drag con mouse → selección visible, text seleccionado correcto

- [x] **T037: Implementar clipboard (Ctrl+Shift+C/V)**
  - RF: RF-29, RF-30
  - `terminal_gui/src/clipboard.rs`: copy selection to clipboard (Ctrl+Shift+C), paste clipboard to session (Ctrl+Shift+V). System clipboard integration
  - Hecho cuando: Ctrl+Shift+C → clipboard tiene texto, Ctrl+Shift+V → texto enviado al panel

## Fase 8: Features avanzadas

- [x] **T038: Implementar cursor shapes**
  - RF: RF-16
  - `terminal_core/src/state.rs` + `terminal_render/src/renderer.rs`: CursorShape enum (Block, Underline, Beam), OSC 22 move-to, renderer dibuja shape correcto
  - Hecho cuando: cursor se dibuja como block/underline/beam según config y OSC 22

- [x] **T039: Implementar window events (OSC 10, 11, 12)**
  - RF: RF-22
  - `terminal_core/src/parser.rs`: Parser reconoce OSC 10 (fg), 11 (bg), 12 (bg) → envía query al GUI → GUI responde con OSC 10/11/12
  - Hecho cuando: terminal query colors → GUI responde con colores actuales

- [x] **T040: Implementar window title e icon name**
  - RF: RF-24
  - `terminal_gui/src/window.rs`: Parser OSC 0, 1, 2 → actualiza tab title y window title
  - Hecho cuando: programa envía OSC 2 "my title" → window title cambia

- [x] **T041: Implementar ligature rendering**
  - RF: RF-19
  - `terminal_render/src/glyph.rs`: rustybuzz shaping → ligature detection → single glyph render
  - Hecho cuando: "fi" → single glyph, configurable on/off

- [x] **T042: Implementar emoji rendering**
  - RF: RF-20
  - `terminal_render/src/glyph.rs`: Emoji font fallback, zwj sequences, skin tone modifiers
  - Hecho cuando: "🦀👨‍💻" → renderizado correcto

## Fase 9: Integración y pulido

- [x] **T043: Integrar todos los crates en la app principal**
  - RF: Todos
  - `terminal/src/main.rs`: Config → Core → Session → Render → GUI. Event loop conecta todo
  - Hecho cuando: app abre, muestra shell, teclas funcionan, tabs y splits funcionan

- [x] **T044: Tests de integración end-to-end**
  - RF: Todos
  - `tests/integration/`: Session completa con PTY mock → parser → buffer → verify output. Config load test. Tab/split lifecycle test
  - Hecho cuando: todos los tests de integración pasan

- [x] **T045: Demo manual — checklist de finalización**
  - RF: Todos
  - Verificar: abrir, crear pestañas, dividir paneles, copiar/pegar, cambiar configuración, ejecutar neofetch/htop
  - Hecho cuando: demo manual pasa sin errores
  - **Nota**: Se fixeó el problema de ventana invisible inicializando GPU base antes del event loop (critical para Wayland). La ventana ahora aparece correctamente.

## Fase 10: Compatibilidad con aplicaciones TUI

- [x] **T046: Corregir despacho de CSI con marcador privado (CL-13)**
  - RF: RF-11, CL-13
  - `terminal_core/src/parser.rs`: recordar el marcador privado (`?`, `>`, `<`, `=`) y no despachar `ESC[>…m`/`>…q` como su comando sin prefijo
  - Hecho cuando: `ESC[>4;1m` no produce SGR 4;1; `ESC[?1049h` sigue entrando en alternate screen

- [x] **T047: Helper de ancho de carácter (RF-44)**
  - RF: RF-44
  - `terminal_core`: añadir dependencia `unicode-width` y helper `char_width(c) -> 0|1|2`
  - Hecho cuando: tests de `a`=1, `日`=2, `🦀`=2, marca combinante=0, control=0

- [x] **T048: Modelo de celda con rol de ancho (RF-44, RF-45)**
  - RF: RF-44, RF-45
  - `terminal_core/src/cell.rs` + `grid.rs`: `Cell.width` (2 cabecera, 1 normal, 0 continuación) y helpers de pareja/limpieza
  - Hecho cuando: tests marcan la hermana como continuación y la limpian sin afectar vecinas

- [x] **T049: Escritura de caracteres anchos y ancho cero (RF-44, CL-9, CL-11)**
  - RF: RF-44, CL-9, CL-11
  - `terminal_core/src/engine.rs::write_char`: avanzar 1/2/0 según ancho, marcar continuación y envolver completo en la última columna
  - Hecho cuando: `日本` ocupa 4 celdas sin partirse; combinante no avanza el cursor; tests de borde en última columna

- [x] **T050: Sobrescritura y erasure limpian celdas hermanas (RF-45, CL-10)**
  - RF: RF-45, CL-10
  - `terminal_core/src/engine.rs` + `buffer.rs`: al escribir/borrar sobre media pareja ancha, limpiar ambas celdas (afecta ECH/EL/ED/IL/DL)
  - Hecho cuando: escribir un angosto sobre la hermana elimina el carácter ancho; no quedan restos en erasures

- [x] **T051: Render de glifos anchos sin solapes (RF-44, RF-45)**
  - RF: RF-44, RF-45
  - `terminal_render/src/renderer.rs`: dibujar el glifo ancho una sola vez y saltar la celda de continuación
  - Hecho cuando: render offscreen de `日本🚀` no muestra solapes ni huecos entre glifos

- [x] **T052: Reset, scroll y resize con continuaciones (RF-45, CL-4)**
  - RF: RF-45, CL-4
  - `terminal_core/src/buffer.rs` + `engine.rs`: clear/scroll/resize nunca dejan celdas de continuación huérfanas
  - Hecho cuando: tras scroll y resize no hay continuaciones sin cabecera (test de invariante)

- [x] **T053: Cadena de fallback de fuentes (RF-46, CL-14)**
  - RF: RF-46, CL-14
  - `terminal_render/src/font.rs` + `glyph.rs`: resolver cada glifo en familia → símbolos → emoji; `.notdef` solo como último recurso
  - Hecho cuando: un glifo de símbolo/emoji se rasteriza con la fuente de respaldo; sin fuente → `.notdef` sin crash

- [x] **T054: Renderizado de emoji (RF-20, RF-46)**
  - RF: RF-20, RF-46
  - `terminal_render/src/glyph.rs`: rasterizar emoji (monocromo/COLR) y confirmar bitmap de 2 celdas
  - Hecho cuando: `🦀` deja de ser una caja negra y ocupa dos celdas

- [x] **T055: Estado de cursor y modo sincronizado (RF-47 a RF-50)**
  - RF: RF-47, RF-48, RF-49, RF-50
  - `terminal_core/src/state.rs`: `Cursor.visible`/`blink`, `SavedCursor`, `TerminalModes.synchronized_output`
  - Hecho cuando: tests de conmutación de visibilidad/parpadeo/sincronizado y de guardado/restauración

- [x] **T056: Parser de `?25` (DECTCEM) y `?2026` (sincronizado) (RF-47, RF-48)**
  - RF: RF-47, RF-48
  - `terminal_core/src/parser.rs`: DECSET/DECRESET para 25 y 2026 → acciones `SetCursorVisible`/`SetSynchronizedOutput`
  - Hecho cuando: tests de parseo y de aplicación al estado

- [x] **T057: Parser y estado de SCP/RCP (RF-50)**
  - RF: RF-50
  - `terminal_core/src/parser.rs` + `engine.rs`: `ESC[s` guarda y `ESC[u` restaura posición, forma y atributos
  - Hecho cuando: mover el cursor, `ESC[s`, escribir, `ESC[u` → vuelve a la posición guardada

- [x] **T058: Parser y estado de DECSCUSR (RF-49)**
  - RF: RF-49
  - `terminal_core/src/parser.rs`: `ESC[Ps q` (intermedio espacio) → forma (bloque/subrayado/barra) y parpadeo según `Ps`
  - Hecho cuando: `Ps` 0–6 producen forma y parpadeo esperados

- [x] **T059: Presentar solo al cerrar la sincronización, con timeout (RF-47, CL-12)**
  - RF: RF-47, CL-12
  - `terminal_core/src/session.rs` + `terminal_gui`/`terminal/src/main.rs`: diferir el present entre `?2026h` y `?2026l`; presentar tras timeout si no llega el `l`
  - Hecho cuando: `?2026h … ?2026l` produce un solo present; sin `l` la pantalla no queda congelada

- [x] **T060: Respetar la visibilidad del cursor en el renderer (RF-48)**
  - RF: RF-48
  - `terminal_render/src/renderer.rs` + `terminal/src/main.rs`: no dibujar el cursor cuando `cursor.visible == false`
  - Hecho cuando: `ESC[?25l` oculta el cursor y `ESC[?25h` lo vuelve a mostrar

- [x] **T061: Respuestas a consultas DSR/DA (RF-51)**
  - RF: RF-51
  - `terminal_core/src/parser.rs` (`ESC[6n`, `ESC[c`) + `terminal/src/main.rs`: componer y escribir la respuesta al PTY
  - Hecho cuando: DSR responde `ESC[<fila>;<columna>R` y DA responde una identificación VT/xterm

- [x] **T062: Verificación end-to-end con una sesión de `opencode` (RF-44 a RF-51)**
  - RF: RF-44 a RF-51, CL-13
  - Regrabar `opencode` por PTY, reinyectarlo en `terminal_core` y renderizarlo offscreen; comparar rejilla contra `pyte`
  - Hecho cuando: la rejilla coincide salvo features fuera de alcance y el frame renderizado no muestra líneas, solapes ni cajas `.notdef`

## Resumen

| Fase | Tareas | RFs cubiertos |
|------|--------|---------------|
| 0: Infraestructura | T001-T002 | — |
| 1: Core MVP | T003-T012 | RF-11 a RF-18, RF-23 |
| 2: PTY & Sessions | T013-T016 | RF-25 a RF-28, CL-1, CL-2 |
| 3: Renderizado | T017-T024 | RF-11 a RF-15, RF-19, RF-20, RF-21, RF-36-37, RF-39, RF-41-42, CL-4, CL-5 |
| 4: Tabs | T025-T027 | RF-2 a RF-5, CL-7 |
| 5: Splits | T028-T030 | RF-6 a RF-10 |
| 6: Config | T031-T034 | RF-31 a RF-43, CL-3 |
| 7: Input & Clipboard | T035-T037 | RF-26, RF-29, RF-30, RF-43 |
| 8: Advanced | T038-T042 | RF-16, RF-19, RF-20, RF-22, RF-24 |
| 9: Integration | T043-T045 | Todos |
| 10: Compatibilidad TUI | T046-T062 | RF-44 a RF-51, CL-9 a CL-14 |

**Total: 62 tareas**
