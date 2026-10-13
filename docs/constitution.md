# Constitución — Terminal en Rust

## Principios

1. **Stack mínimo.** Solo crates con mantenimiento activo y licencia MIT/Apache-2.0. Preferir winit+wgpu o crossterm según alcance.
2. **Spec > código.** Nada se implementa sin estar en `spec.md`. Los cambios van primero a la spec.
3. **Lógica separada de la UI.** El motor de la terminal (pantalla, buffer, parser de secuencias) vive en un crate `terminal_core` sin dependencias de GUI.
4. **Tests obligatorios.** Cada requisito funcional tiene al menos un test. `cargo test` debe pasar siempre (CI verde).
5. **Datos portables.** Configuración en `$XDG_CONFIG_HOME/terminal/`. Sin estado global oculto.
6. **Código en inglés.** Nombres, docs y mensajes en inglés. Los mensajes al usuario van por i18n, no hardcodeados.
