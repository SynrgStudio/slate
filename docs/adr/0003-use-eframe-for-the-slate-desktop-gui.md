# ADR-0003: Use eframe for the Slate desktop GUI

Status: proposed
Date: 2026-05-24
Supersedes: none
Superseded by: none

## Context

Slate is a minimal GUI notepad with no menubar or toolbar and a dark terminal-inspired look.

Evidence: Cargo.toml, README.md

## Decision

Use Rust with eframe configured with default fonts, glow, and x11 features. This ADR is proposed and must be reviewed before acceptance.

## Consequences

The app can remain a native Rust GUI application, but Linux/headless environments may need display dependencies.

## Links

- CONTEXT.md
- STATE.md
- ACTIVE_QUEUE.md
- AUTONOMOUS_EXECUTION.md
- Cargo.toml
- README.md
