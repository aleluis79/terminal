---
name: ssd-tasks
description: usa esta skill cuando el usuario pida generar la lista de tareas de implementación a partir de spec.md y plan.md. Genera specs/XXX-YYY/tasks.md con tareas pequeñas, ordenadas por dependencia, con RF que cubre y línea "Hecho cuando:".
---

# ssd-tasks

A partir de `spec.md` y `plan.md`, genera `specs/XXX-YYY/tasks.md`:

- Tareas pequeñas (máx. 20-30 min cada una).
- En orden de dependencia.
- Cada tarea debe incluir los RF (requisitos funcionales) que cubre.
- Cada tarea debe tener una línea "Hecho cuando:" verificable.
- Usa checkboxes (`- [ ]`).

## Reglas

- No implementes código, solo genera la lista de tareas.
- Asegúrate de que todas las tareas estén ordenadas lógicamente según las dependencias.
- Cada tarea debe ser autónoma y verificable.
