# ADR-0001: ThreadSuite bootstrap

Status: proposed
Date: 2026-05-24
Supersedes: none
Superseded by: none

## Context

ThreadSuite performed a bounded project inventory and generated durable project knowledge scaffolding.
Detected project type: Rust/Cargo.
Project purpose: Minimal GUI notepad inspired by a clean terminal look..
This decision is inferred from bootstrap behavior and should be reviewed by the user before acceptance.

## Decision

Use ThreadSuite as the project bootstrap entrypoint for bounded inventory, continuity/context files, memory capsules, and ADR candidates. This ADR is proposed and must be reviewed before acceptance.

## Consequences

Threadwell can start future sessions from durable project knowledge before broad repository exploration. Generated facts remain inferred until reviewed.

## Links

- CONTEXT.md
- STATE.md
- ACTIVE_QUEUE.md
- AUTONOMOUS_EXECUTION.md
