# Slate Philosophy

Slate is a local-first, commandline-first text workspace for writing, editing, capturing, and gradually organizing knowledge.

Slate is a normal text editor first. Its knowledge-work features should grow from plain files, fast commands, and lightweight conventions rather than from a mandatory database, graph, plugin ecosystem, or permanent UI chrome.

## What Slate is

Slate is:

- a fast personal text editor
- a terminal-like writing workspace
- a local Markdown knowledge environment
- a capture-first inbox for thoughts, notes, tasks, and drafts
- a command-driven tool that can still be used like a normal editor

Slate should feel closer to a small, focused terminal editor with useful knowledge workflows than to a large productivity suite.

## Non-negotiable principles

1. **Text files are the source of truth.**
   Markdown and plain text files remain authoritative. Any index, cache, or database must be rebuildable.

2. **Normal editing comes first.**
   Typing, opening, saving, selecting, and navigating text must stay simple and predictable. Advanced features must not make basic editing weird.

3. **Commands beat chrome.**
   The command line is the primary interface. The command palette and modals are browsers for discoverability and long lists, not replacements for fast commands.

4. **Capture first, organize later.**
   Slate should make it easy to write something down now and progressively organize it only when needed.

5. **Organization is progressive, not mandatory.**
   Folders, scratch files, daily notes, wiki links, tags, and indexes should be optional layers over normal files.

6. **Local-first by default.**
   Slate works on local files without accounts, sync services, servers, or cloud assumptions.

7. **UI appears when useful, then gets out of the way.**
   Use focused modals for large workflows such as files, help, and future search results. Avoid permanent sidebars, toolbars, and noisy panels.

8. **Markdown stays simple.**
   Slate may improve Markdown editing and preview, but should not become a heavy renderer with editing bolted on.

9. **Databases are caches, not truth.**
   SQLite or other indexes may exist for speed, search, backlinks, and link resolution, but they must never become the canonical note store.

10. **Configuration is intentional.**
    Prefer curated defaults and a small number of meaningful settings over infinite configurability.

## Anti-goals

Slate is not:

- an Obsidian clone
- a Logseq clone
- an Emacs distribution
- a plugin platform first
- a graph database UI
- a sync/productivity suite
- a project-management system
- a Markdown renderer with editing as a secondary feature
- a tool that requires a vault before it can edit normal files

These ideas can inspire individual interactions, but Slate should not inherit their complexity by default.

## Feature decision rules

Before adding or expanding a feature, ask:

- Does this preserve normal text editing?
- Does it work on plain local files?
- Can it be commandline-first?
- Does it avoid permanent UI clutter?
- Is it useful without a vault?
- If it uses an index/cache, can that cache be rebuilt?
- Does it help writing, capture, navigation, editing, or retrieval?
- Can it start simple and grow progressively?

If a feature fails several of these questions, it should be simplified, delayed, or rejected.

## UI direction

Slate's UI should remain minimal and terminal-like:

- no menubar
- no toolbar
- statusbar plus command line/minibuffer
- commandline-first workflows
- command palette as a browser/discovery tool
- Slate-owned modals for large lists and focused tasks
- normal typing by default
- no permanent Vim mode
- transient layers for power workflows when useful

Long-list workflows belong in dedicated modals, not cramped bottom lists. Bottom transient UI should support quick input and status, not become a second application.

## Knowledge-work direction

Slate's knowledge features should be built as small layers over normal files.

### Scratch

Scratch is the fast inbox: capture now, organize later. It should support quick append and sending selected text/current lines without becoming a full task system.

### Daily notes

Daily notes are plain Markdown files, probably under a configurable notes folder. They should be easy to open and append to, not a calendar application.

### Vault

A vault is optional. It is just a normal folder that Slate treats as a useful root for scratch, daily notes, ideas, projects, wiki links, and search.

Slate must remain useful without a vault.

### Wiki links

Wiki links should improve navigation through text. They should start with simple `[[note]]` / `[[path/file.md:line:column]]` behavior before anything graph-like.

### Search and backlinks

Search should prioritize fast textual results and direct navigation. Backlinks can use an index, but the index must be rebuildable from Markdown files.

### Indexing

Indexes exist for speed and retrieval. They must be optional implementation details, not user-facing storage formats.

## The preferred shape of Slate

Slate should feel like:

> Open files quickly, write normally, capture instantly, navigate with commands, organize progressively, and keep everything as local text.

If a new feature reinforces that sentence, it probably belongs. If it fights that sentence, it probably does not.
