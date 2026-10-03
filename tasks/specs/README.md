# Specs Governance

This directory holds living roadmaps and pre-implementation feature plans for Hephaestus LLM Studio.

## What Is a Valid Spec

A **valid spec** is one of:

1. **Living Roadmap**: A multi-wave technical plan covering active development for a subsystem (backend service, engine, infra layer). Must track:
   - Waves (phases with clear exit criteria)
   - Actionable items with assignable owners
   - Continuous execution plan, not snapshot

2. **Pre-Implementation Feature Plan**: A specification for a new feature *before* code lands. Must include:
   - User story or capability statement
   - Acceptance criteria
   - Non-functional constraints (performance, security, scalability)
   - No implementation yet (ideation + design only)

## What Is NOT a Valid Spec

- **Audit-only snapshot**: Point-in-time findings without execution plan. Examples: "memory audit of qwen-image on 2026-03-15". These go to `docs/archive/specs/` once findings are triaged and assigned.
- **Multi-domain dumping ground**: A single file mixing orchestrator + manager + API + telemetry + infra findings without clear ownership or phase plan (see *Multi-Domain Anti-Pattern* below).

## Active Specs (4)

| File | Domain | ID Namespace | Scope |
|------|--------|--------------|-------|
| `consolidacao-auditoria-roadmap.md` | Backend | `RD-*` | Waves 0–5: orchestrator, manager, api-principal modularization & autonomy |
| `engines-auditoria-global.md` | ML Engines | simple numeric | Roadmap & findings for trainer-difusão, trainer-yolo, and dependent engines |
| `infra-pendencias.md` | Infrastructure | `INFRA-NN` + `P0–P3` | Operational blockers: Docker Compose, Dockerfiles, networking (9 open items) |
| `treino-flux-performance.md` | Performance | `P1–P5` | Flux training throughput optimization; includes comparative context vs Qwen-Image |

## Closure Rule

When a spec has zero open items:

1. Verify no residual findings exist for other domains (if any, migrate to their spec first)
2. Run `git mv tasks/specs/<filename> docs/archive/specs/<filename>`
3. Add a header note to the moved file:
   ```
   > Archived 2026-09-26 | All items closed or migrated to [target-spec.md]
   ```

This preserves git history and marks the lifecycle clearly.

## Anti-Duplication Rule

**Before creating a new spec**, search `tasks/specs/` for active coverage of your subsystem.

**Real example of what NOT to do:**  
Found today: identical orchestrator/manager/api-principal roadmap items appeared in **4 separate files** with different ID namespaces (`T-M01`, `T-01`, `P0-1`, `1.1`), each maintained independently. This caused:
- Rework duplication (same item fixed in parallel in multiple files)
- Ownership ambiguity (which file is authoritative?)
- Lost context (merging discoveries across 4 sources)

**Instead:** All backend service findings → **one** `consolidacao-auditoria-roadmap.md` with unified `RD-*` namespace. One source of truth per subsystem.

## Multi-Domain Anti-Pattern

Avoid specs that mix e.g. backend findings + engine tuning + infra blockers + telemetry in a single file without clear domaining.

Why:
- No single owner (backend engineer, ML engineer, SRE all "own" parts)
- Impossible execution plan (each domain has different cadence, reviewers, rollout strategy)
- Leads to archival as unsorted audit rather than execution

Each spec MUST serve one subsystem or one feature milestone.
