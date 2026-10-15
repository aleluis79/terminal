# Spec 001 — Terminal Emulator

## Contexto y objetivo
Construir un emulador de terminal moderno, rápido y portable que ejecute shells y programas de forma fiable. El objetivo es ofrecer una experiencia de uso completa (pestañas, paneles, configuración) sin depender de un entorno de escritorio específico.

## Usuarios / actores
- **Desarrollador:** usa la terminal para ejecutar shells, editores, herramientas de línea de comandos.
- **Sistema:** envía datos al buffer de la terminal y recibe eventos de teclado/ratón.

## Historias de usuario
- H1: Como desarrollador quiero abrir una ventana con una pestaña para ejecutar comandos.
- H2: Como desarrollador quiero crear nuevas pestañas para tener múltiples sesiones.
- H3: Como desarrollador quiero dividir una pestaña en paneles para ver múltiples sesiones simultáneamente.
- H4: Como desarrollador quiero cambiar la fuente y su tamaño para ajustar la legibilidad.
- H5: Como desarrollador quiero que la configuración se lea de un archivo para no repetir argumentos.
- H6: Como desarrollador quiero elegir la shell (cmd, powershell, wsl, …) por configuración o al crear la pestaña, para no depender de la shell por defecto del sistema.
- H7: Como desarrollador quiero que las teclas especiales (Escape, Supr, …) se envíen con la codificación que esperan las aplicaciones de terminal, para que los editores TUI (vim, nano) respondan bien.

## Requisitos funcionales (criterios de aceptación en notación EARS)

### Ventana y pestañas
- RF-1: CUANDO el usuario inicia la aplicación, EL SISTEMA abre una ventana con una pestaña por defecto.
- RF-2: DONDE el usuario presiona la tecla de crear pestaña (por defecto Ctrl+Shift+N), ENTONCES EL SISTEMA crea una nueva pestaña con una sesión de shell.
- RF-3: DONDE el usuario presiona la tecla de cerrar pestaña (por defecto Ctrl+Shift+W), ENTONCES EL SISTEMA cierra la pestaña activa y enfoca la adyacente.
- RF-4: DONDE hay más de una pestaña, EL SISTEMA muestra una barra de pestañas con el título de cada una.
- RF-5: DONDE el usuario hace clic en una pestaña de la barra, ENTONCES EL SISTEMA enfoca esa pestaña.

### Paneles
- RF-6: DONDE el usuario presiona la tecla de dividir horizontal (por defecto Ctrl+Shift+↓), ENTONCES EL SISTEMA divide el panel activo en dos paneles verticales (uno arriba, otro abajo).
- RF-7: DONDE el usuario presiona la tecla de dividir vertical (por defecto Ctrl+Shift+→), ENTONCES EL SISTEMA divide el panel activo en dos paneles horizontales (uno a la izquierda, otro a la derecha).
- RF-8: DONDE el usuario presiona la tecla de cerrar panel (por defecto Ctrl+Shift+X), ENTONCES EL SISTEMA cierra el panel activo y enfoca un panel adyacente.
- RF-9: DONDE el usuario arrastra el divisor entre paneles, ENTONCES EL SISTEMA redimensiona los paneles adyacentes proporcionalmente.
- RF-10: DONDE el usuario presiona la tecla de navegar entre paneles (por defecto Ctrl+Shift+H/J/K/L), ENTONCES EL SISTEMA enfoca el panel vecino en esa dirección.

### Motor de terminal
- RF-11: EL SISTEMA soporta la norma ECMA-48 y las secuencias ANSI/VT100/VT200/XTerm completas.
- RF-12: EL SISTEMA soporta colores 16, colores 256 (palette) y colores RGB (24-bit).
- RF-13: EL SISTEMA soporta estilos de texto: negrita, cursiva, subrayado, inverso, oculto, strike.
- RF-14: EL SISTEMA soporta el alternate screen (secuencias `?1049`).
- RF-15: EL SISTEMA soporta scrollback de al menos 10000 líneas.
- RF-16: EL SISTEMA soporta cursor con formas: bloque, línea, subrayado, y move-to (OSC 22).
- RF-17: EL SISTEMA soporta bracketed paste mode (OSC 200).
- RF-18: EL SISTEMA soporta OSC-8 para hyperlinks.
- RF-19: EL SISTEMA soporta ligaduras tipográficas (configurable).
- RF-20: EL SISTEMA soporta renderizado de emoji (configurable).
- RF-21: EL SISTEMA soporta el Kitty graphics protocol (inline images).
- RF-22: EL SISTEMA soporta window events (OSC 10, 11, 12).
- RF-23: EL SISTEMA soporta bell audible y bell visual.
- RF-24: EL SISTEMA soporta title y icon name del window (OSC 0, OSC 1, OSC 2).

### Entrada y salida
- RF-25: EL SISTEMA lee datos del child process (pty) y los renderiza en el panel activo.
- RF-26: EL SISTEMA envía al child process todo evento de teclado recibido.
- RF-27: DONDE el usuario presiona Ctrl+C, ENTONCES EL SISTEMA envía SIGINT al child process.
- RF-28: DONDE el usuario presiona Ctrl+D, ENTONCES EL SISTEMA envía EOF al child process.
- RF-29: DONDE el usuario selecciona texto con el ratón y presiona Ctrl+Shift+C, ENTONCES EL SISTEMA copia la selección al clipboard.
- RF-30: DONDE el usuario presiona Ctrl+Shift+V, ENTONCES EL SISTEMA envía al panel activo el contenido del clipboard.

### Configuración
- RF-31: EL SISTEMA lee un archivo de configuración en formato TOML al iniciar.
- RF-32: DONDE el archivo de configuración existe, ENTONCES EL SISTEMA aplica los valores definidos como valores por defecto.
- RF-33: DONDE un valor se pasa por argumento CLI, ENTONCES EL SISTEMA sobreescribe el valor de la configuración de archivo.
- RF-34: EL SISTEMA genera un archivo de configuración por defecto si no existe uno existente.
- RF-35: DONDE el archivo de configuración contiene un error de sintaxis, ENTONCES EL SISTEMA muestra un mensaje de error y usa valores por defecto.

### Configuración específica
- RF-36: DONDE la configuración define `font.family`, EL SISTEMA usa esa fuente para renderizar texto.
- RF-37: DONDE la configuración define `font.size`, EL SISTEMA usa ese tamaño en puntos para renderizar texto.
- RF-38: DONDE la configuración define `window.title`, ENTONCES EL SISTEMA usa ese texto como título de la ventana.
- RF-39: DONDE la configuración define `window.dimensions.columns` y `window.dimensions.rows`, ENTONCES EL SISTEMA crea la ventana con esa cantidad de celdas.
- RF-40: DONDE la configuración define `window.opacity` (0.0–1.0), ENTONCES EL SISTEMA aplica esa opacidad a la ventana.
- RF-41: DONDE la configuración define `cursor.style`, ENTONCES EL SISTEMA renderiza el cursor con ese estilo (block, underline, beam).
- RF-42: DONDE la configuración define `colors.primary.background` y `colors.primary.foreground`, ENTONCES EL SISTEMA usa esos colores para el tema.
- RF-43: DONDE la configuración define `keyboard.bindings`, ENTONCES EL SISTEMA usa esas combinaciones en vez de las por defecto.

### Compatibilidad con aplicaciones TUI

- RF-44: EL SISTEMA determina el ancho de cada carácter según Unicode (`unicode-width`): los caracteres de ancho doble (CJK, emoji anchos) ocupan dos celdas contiguas de la rejilla y desplazan el cursor dos columnas; los de ancho cero (marcas combinantes) no ocupan celda ni desplazan el cursor.
- RF-45: DONDE un carácter se escribe o se borra sobre una de las celdas de un carácter de ancho doble, ENTONCES EL SISTEMA limpia ambas celdas para que no queden restos del carácter anterior.
- RF-46: DONDE la fuente configurada no contiene el glifo de un carácter, EL SISTEMA lo busca en una cadena de fuentes de respaldo (símbolos y emoji) y solo dibuja el glifo `.notdef` si ninguna lo contiene.
- RF-47: DONDE el programa activa la salida sincronizada (`ESC[?2026h`), EL SISTEMA acumula las escrituras y solo presenta el frame al recibir `ESC[?2026l`, sin mostrar estados intermedios.
- RF-48: DONDE el programa oculta el cursor (`ESC[?25l`), ENTONCES EL SISTEMA deja de dibujarlo; DONDE lo vuelve a mostrar (`ESC[?25h`), ENTONCES EL SISTEMA vuelve a dibujarlo.
- RF-49: DONDE el programa envía el estilo de cursor DECSCUSR (`ESC[Ps q`), ENTONCES EL SISTEMA dibuja el cursor con la forma indicada (bloque, subrayado o barra) y con parpadeo o fijo según `Ps`.
- RF-50: DONDE el programa guarda el cursor (`ESC[s`), ENTONCES EL SISTEMA recuerda su posición y forma; DONDE lo restaura (`ESC[u`), ENTONCES EL SISTEMA vuelve a esa posición.
- RF-51: DONDE el programa consulta la posición del cursor (`ESC[6n`), EL SISTEMA responde `ESC[<fila>;<columna>R`; DONDE consulta los colores activos (OSC 10/11/12 con `?`), EL SISTEMA responde el color correspondiente; DONDE consulta los atributos del dispositivo (`ESC[c`), EL SISTEMA responde una identificación de terminal VT/xterm.

### Paneles

- RF-52: DONDE una pestaña tiene más de un panel, EL SISTEMA dibuja una línea divisoria visible en el hueco entre paneles adyacentes, con el color de acento del tema.

### Desplazamiento

- RF-53: DONDE el usuario gira la rueda del ratón sobre un panel, o pulsa Shift+PageUp/PageDown/Home/End con el panel enfocado, EL SISTEMA desplaza la vista verticalmente por el historial de scrollback (hasta el límite de RF-15) sin modificar el contenido del buffer.
- RF-54: MIENTRAS la vista está desplazada hacia el historial, EL SISTEMA no dibuja el cursor; CUANDO el programa escribe nueva salida, EL SISTEMA vuelve al final del buffer.
- RF-55: DONDE el programa envía Índice (`ESC D`), EL SISTEMA baja el cursor una fila y, si ya está en la última, desplaza el contenido una línea hacia arriba; DONDE envía Índice Inverso (`ESC M`), EL SISTEMA sube el cursor una fila y, si ya está en la primera, desplaza el contenido una línea hacia abajo (insertando una línea en blanco arriba); DONDE envía Next Line (`ESC E`), EL SISTEMA baja una fila y mueve el cursor a la primera columna.
- RF-56: DONDE el usuario gira la rueda del ratón con la pantalla alterna activa y la aplicación no ha activado el mouse reporting, ENTONCES EL SISTEMA envía a la aplicación las flechas arriba/abajo equivalentes (alternate scroll), en vez de desplazar el scrollback; DONDE la pantalla alterna no está activa, ENTONCES EL SISTEMA aplica RF-53.
- RF-57: DONDE el programa define una región de scroll (`ESC[Pt;Pbr`), EL SISTEMA limita los desplazamientos verticales (salto de línea, índice, índice inverso, insertar/borrar línea) a las filas `Pt`..`Pb` y deja intactas las filas fuera de la región; DONDE la región es inválida (`Pt >= Pb`), EL SISTEMA ignora la secuencia. La región solo alimenta el scrollback cuando abarca toda la pantalla.
- RF-58: EL SISTEMA pinta la barra de pestañas y las pestañas inactivas con el color de fondo del tema (`colors.primary.background`); la pestaña activa se dibuja como un chip de color más oscuro (`ansi.black`) con esquinas redondeadas.
- RF-59: DONDE el usuario hace clic en la `×` de la derecha de una pestaña, EL SISTEMA cierra esa pestaña con sus paneles; DONDE era la última pestaña, EL SISTEMA cierra la aplicación (CL-7).
- RF-60: EL SISTEMA identifica la ventana con el nombre de aplicación `terminal` (app_id de Wayland / `WM_CLASS` de X11) y provee `assets/terminal.desktop` con `Icon=terminal` e `StartupWMClass=terminal` junto con `assets/terminal.png`, de modo que el entorno de escritorio muestre el nombre y el icono de la aplicación en la barra de tareas.
- RF-61: CUANDO el programa selecciona el juego de caracteres DEC Special Graphics (`ESC ( 0`), EL SISTEMA dibuja los caracteres ASCII de la zona `0x60`–`0x7e` y el `_` como sus glifos de líneas y símbolos (`q`→`─`, `x`→`│`, `l`→`┌`, `j`→`┘`, `n`→`┼`, `f`→`°`, `~`→`·`, …) en vez de las letras; DONDE el programa selecciona el juego ASCII (`ESC ( B`), EL SISTEMA vuelve a dibujar las letras. El juego del registro G1 (`ESC ) 0`) se activa con SO (`0x0E`) y se desactiva con SI (`0x0F`).

### Selección de shell

- RF-62: DONDE la configuración define `shell.program` y opcionalmente `shell.args`, ENTONCES EL SISTEMA lanza ese programa con esos argumentos en cada sesión nueva, en lugar de la shell por defecto del sistema.
- RF-63: DONDE el usuario pasa `--shell` como argumento CLI, ENTONCES EL SISTEMA usa ese programa para las sesiones nuevas en vez del definido en `shell.program` (RF-33).
- RF-64: DONDE la configuración define `shell.profiles` (lista de perfiles, cada uno con `name`, `program` y `args` opcionales), EL SISTEMA usa esos perfiles como las shells elegibles por el usuario, y el primer perfil (o `shell.program`, si existe) es el perfil por defecto.
- RF-65: DONDE no hay perfiles configurados y el SISTEMA corre en Windows, EL SISTEMA ofrece los perfiles `cmd`, `powershell` y `wsl` para los ejecutables que existan en el sistema; DONDE corre en Linux, EL SISTEMA ofrece un único perfil por defecto con la shell del usuario (`$SHELL`).
- RF-66: DONDE el usuario presiona el atajo del perfil N (por defecto Ctrl+Shift+1 … Ctrl+Shift+9), ENTONCES EL SISTEMA crea una pestaña nueva con una sesión del perfil N; el atajo de nueva pestaña (RF-2) crea la sesión con el perfil por defecto.
- RF-67: DONDE el programa de la shell elegida no existe o falla al lanzarse, ENTONCES EL SISTEMA muestra un mensaje de error y abre la sesión con la shell por defecto del sistema, sin dejar la pestaña vacía.

### Codificación de teclas especiales

- RF-68: CUANDO el usuario presiona Escape, EL SISTEMA envía al child process el byte `ESC` (`0x1b`) y ninguna secuencia adicional.
- RF-69: CUANDO el usuario presiona Supr (`Delete`), EL SISTEMA envía al child process la secuencia `ESC[3~`; DONDE la tecla lleva modificadores (Alt o Ctrl), EL SISTEMA envía `ESC[3;N~` con el modificador codificado.

### Shell al dividir paneles

- RF-70: CUANDO el usuario divide un panel (RF-6, RF-7), EL SISTEMA crea la sesión del panel nuevo con la misma shell que está corriendo en el panel dividido, en lugar del perfil por defecto (excepción a RF-62 para las sesiones creadas al dividir).

## Requisitos no funcionales
- Rendimiento: renderizado a 60 FPS con 10000 líneas de texto visibles.
- Memoria: uso inferior a 100 MB en carga normal (50000 líneas de scrollback).
- Portabilidad: compila y ejecuta en Linux (x86_64, aarch64) y Windows (x86_64).
- Idioma: código y documentación en inglés (según constitución).

## Casos límite
- CL-1: Child process termina inesperadamente — EL SISTEMA muestra un mensaje y termina la pestaña.
- CL-2: Child process produce datos binarios no UTF-8 — EL SISTEMA reemplaza bytes inválidos por el carácter U+FFFD sin crash.
- CL-3: Archivo de configuración con formato válido pero valores inválidos (ej. fuente inexistente, tamaño 0) — EL SISTEMA ignora el valor inválido y usa el por defecto.
- CL-4: Ventana redimensionada rápidamente — EL SISTEMA re-renderiza sin parpadeo ni pérdida de scrollback.
- CL-5: Fuente no soporta algún glyph (emoji, ligadura) — EL SISTEMA fallback a la siguiente fuente en la familia.
- CL-6: Se crean muchas pestañas rápidamente — EL SISTEMA no pierde ninguna sesión y no duplica procesos.
- CL-7: Pestaña única cerrada — EL SISTEMA cierra la aplicación (no queda ventana vacía).
- CL-8: Panel con child process que escribe más rápido de lo que se renderiza — EL SISTEMA no pierde datos en el scrollback.
- CL-9: Carácter de ancho doble en la última columna de la línea — EL SISTEMA lo envuelve completo a la línea siguiente, sin partirlo entre dos líneas.
- CL-10: Carácter de ancho 1 escrito sobre la mitad (celda hermana) de un carácter de ancho doble — EL SISTEMA borra el carácter ancho completo antes de escribir el nuevo.
- CL-11: Marca combinante sin carácter base (inicio de línea o sobre una celda vacía) — EL SISTEMA la ignora sin desplazar el cursor y sin crash.
- CL-12: Salida sincronizada activada (`?2026h`) y el programa termina sin enviar `?2026l` — EL SISTEMA presenta el estado pendiente tras un timeout, sin dejar la pantalla congelada.
- CL-13: Secuencia CSI con marcador privado no soportado (`ESC[>…`) — EL SISTEMA la consume completa y no la interpreta como su comando sin prefijo (regresión de RF-11).
- CL-14: Emoji o símbolo que ninguna fuente del sistema contiene — EL SISTEMA dibuja `.notdef` sin crash.
- CL-15: La vista se desplaza más allá de la línea más antigua del historial — EL SISTEMA se detiene en la más antigua y no falla.
- CL-16: Rueda del ratón con la pantalla alterna activa y sin mouse reporting — EL SISTEMA envía flechas arriba/abajo a la aplicación (RF-56) y no desplaza el scrollback (la pantalla alterna no tiene historial).
- CL-17: Índice Inverso (`ESC M`) en la primera fila de la pantalla principal — EL SISTEMA inserta una línea en blanco arriba, desplaza el contenido hacia abajo y descarta la última línea, sin añadir nada al scrollback.
- CL-18: Índice (`ESC D`) en la última fila de la pantalla activa — EL SISTEMA desplaza el contenido hacia arriba: en la pantalla principal la línea que sale por arriba entra al scrollback y en la alterna se descarta.
- CL-19: Rueda del ratón en pantalla alterna con mouse reporting activo — EL SISTEMA reenvía el evento al programa (fuera de alcance actual, solo selección de texto; ver "Fuera de alcance").
- CL-20: Región de scroll que no abarca toda la pantalla y salto de línea en el margen inferior — EL SISTEMA desplaza solo las filas de la región, deja intactas las de arriba/abajo y no alimenta el scrollback.
- CL-21: Región de scroll e Índice Inverso en el margen superior — EL SISTEMA desplaza la región hacia abajo; con el cursor fuera de la región, solo lo mueve sin desplazar.
- CL-22: `ESC[Pt;Pbr` con `Pt >= Pb` — EL SISTEMA ignora la secuencia y mantiene la región anterior (pantalla completa).
- CL-23: Programa que deja activo DEC Special Graphics y luego escribe texto ASCII normal — EL SISTEMA solo aplica el dibujo de líneas mientras el juego está seleccionado; al recibir `ESC ( B`, `SI` o un reset (`ESC c`) vuelve a dibujar las letras.

- CL-24: `shell.program` (o el `program` de un perfil) no existe o no es ejecutable — EL SISTEMA muestra un mensaje de error y abre la sesión con la shell por defecto del sistema (RF-67).
- CL-25: `shell.args` (o los `args` de un perfil) con espacios o comillas — EL SISTEMA los pasa como argumentos independientes, sin reinterpretarlos como una línea de comandos.
- CL-26: Perfil de `shell.profiles` sin `name` o sin `program` — EL SISTEMA descarta ese perfil y sigue con los demás.
- CL-27: El usuario presiona el atajo de un perfil inexistente (p.ej. Ctrl+Shift+5 con solo 2 perfiles) — EL SISTEMA no abre ninguna pestaña.
- CL-28: La shell elegida arranca y termina al instante (p.ej. `wsl.exe` sin distribución por defecto) — EL SISTEMA aplica CL-1 (mensaje de salida y cierra la pestaña).
- CL-29: `wsl.exe` no está instalado — EL SISTEMA no ofrece el perfil `wsl` por defecto; si el usuario lo configura explícitamente, aplica CL-24.
- CL-30: Escape o Supr con modificadores (Alt/Ctrl) — EL SISTEMA envía `ESC` a secas para Escape y `ESC[3;N~` para Supr, sin generar secuencias mal formadas como `ESC[[D` ni `ESC[~` (RF-68, RF-69).
- CL-31: El panel dividido corre la shell por defecto porque su shell elegida falló al arrancar (RF-67) — EL SISTEMA crea el panel nuevo con esa misma shell (la que realmente corre), sin repetir el error.
- CL-32: Divisiones sucesivas y cierre del panel original — EL SISTEMA hereda la shell del panel que se divide en cada operación, de modo que la shell elegida se conserva en los paneles derivados (RF-70).

## Fuera de alcance
- Soporte de macOS.
- Soporte de Wayland (solo X11 en Linux).
- Sincronización de paneles (paneles sincronizados entre sí).
- Soporte de temas dinámicos / cambio de colores en runtime.
- Soporte de daemon / persistencia de sesiones (como tmux).
- Soporte de GPU-accelerated rendering (software rendering con wgpu en CPU es aceptable).
- Soporte de mouse reporting avanzado (solo selección de texto).
- Soporte de terminal multiplexor integrado.
- Clipboard nativo (sin modifiers) ni drag-and-drop de archivos.
- Detección automática de las distribuciones WSL instaladas (se elige `wsl.exe` y sus argumentos).
- Editor de perfiles de shell dentro de la aplicación (se editan en el archivo de configuración).

## Criterios de finalización
- Todos los RF (1–70) tienen al menos un test automatizado o verificación manual documentada.
- `cargo test` pasa en Linux y Windows.
- Demo manual: abrir, crear pestañas, dividir paneles, copiar/pegar, cambiar configuración, y ejecutar `neofetch` o `htop` correctamente.

## Dudas abiertas
- ~~¿clipboard nativo?~~ → Solo Ctrl+Shift+C/V.
- ~~¿drag-and-drop de archivos?~~ → Sin drag-and-drop.
- ~~¿hot-reload de configuración?~~ → Basta con reiniciar.
- ¿Corregir el orden de modificadores de las flechas (`KeyEvent::Arrow` manda ctrl como `3` y alt como `5`, invertido respecto a xterm y al resto de teclas especiales)? → Requiere un RF propio; pendiente de decidir.
