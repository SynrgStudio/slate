# ADR-0002: Complete ThreadSuite is the default bootstrap profile

Status: proposed
Date: 2026-05-24
Supersedes: none
Superseded by: none

## Context

ThreadSuite supports complete and lite profiles.
Detected project type: Rust/Cargo.
Project purpose: Minimal GUI notepad inspired by a clean terminal look..
This decision is inferred from bootstrap behavior and should be reviewed by the user before acceptance.

## Decision

Make bare `/threadsuite` run the complete profile and require `lite` for the intentionally smaller bootstrap. This ADR is proposed and must be reviewed before acceptance.

## Consequences

The default gives the agent the richest safe project intelligence. Users can still choose lite when they want less generated scaffolding.

## Links

- CONTEXT.md
- STATE.md
- ACTIVE_QUEUE.md
- AUTONOMOUS_EXECUTION.md
