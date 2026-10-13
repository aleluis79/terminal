---
name: ssd-validation
description: usa esta skill cuando el usuario pida validar una implementación contra la especificación. Recorre requisitos RF-1 a RF-n, indica qué test lo cubre, resultado, y da un veredicto final sobre si la spec está cumplida.
---

# ssd-validation

Recorre `specs/XXX-YYY/spec.md` requisito por requisito (RF-1 a RF-n).

Para cada uno indica:
- Qué test lo cubre.
- El resultado de ejecutarlo.

Si algún RF no está cubierto o falla, dilo claramente.

Después comprueba los criterios de finalización y dame un veredicto: ¿la spec está cumplida?
