---
name: ssd-planning
description: usa esta skill cuando el usuario pida generar el plan de implementación de una especificación. Lee docs/constitution.md y specs/XXX-YYY/spec.md, y genera specs/XXX-YYY/plan.md con estructura de módulos, modelo de datos, decisiones justificadas y estrategia de tests, sin escribir código.
---

# ssd-planning

Lee `docs/constitution.md` y `specs/XXX-YYY/spec.md`. NO escribas código.

Genera `specs/XXX-YYY/plan.md` con:

1. **Estructura de módulos**: organización del código y responsabilidades.
2. **Modelo de datos**: entidades, relaciones y persistencia.
3. **Decisiones justificadas**: alternativas consideradas y la descartada con razón.
4. **Estrategia de tests**: tipos de tests y cobertura sobre los requisitos.

## Reglas

- Todo debe respetar la constitución y cubrir todos los RF (requisitos funcionales).
- Marca qué RF cubre cada parte del plan.
- NO escribas código de implementación, solo el plan arquitectónico y de diseño.
