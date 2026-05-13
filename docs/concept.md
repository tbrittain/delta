# delta — Concept

> A high-performance terminal-based semantic Git diff review tool optimized for AI-assisted development workflows.

---

## The Problem

When working in an agentic coding loop, the human review step is friction-heavy.

**Current workflow:**
1. Go into agentic coding loop
2. Have AI check in
3. Open the branch the AI is working on
4. Review proposed changes — manually hunt through files
5. Copy and paste file paths for files with comments
6. Write a long message back to the AI about concerns
7. AI fixes the issue → back to step 1

Steps 3–6 are the target. They require context-switching out of the terminal, manually locating relevant hunks, and composing unstructured feedback without anchoring it to specific code.

**New workflow with delta:**
1. Go into agentic coding loop
2. Have AI check in
3. Run `delta main` — opens a TUI showing all changed files (branch vs trunk)
4. Navigate diffs, leave comments on specific hunks/code regions
5. Close delta → feedback exported as structured markdown/JSON with file + line context
6. Feed the export back to the AI
7. AI fixes the issue → back to step 1

---

## What It Is

- A terminal-first, local-first diff review tool
- An orchestration layer between human reviewers and AI coding agents
- A human feedback capture system for agentic coding loops
- Git-centric: operates on committed branch changes vs a base ref

## What It Is Not

- A full IDE or text editor
- A merge tool
- A GitHub/GitLab replacement
- A persistent code review platform
- A general-purpose diff engine research project
- A `git difftool` driver

---

## Invocation

```bash
delta main              # current branch vs main (to defaults to HEAD)
delta origin/main       # current branch vs remote main
delta abc1234 def5678   # diff between two arbitrary commits
```

delta is a standalone command that shells out to git. The agent's commit/staging workflow is entirely outside delta's scope — delta only ever sees already-committed changes.

---

## Key Design Principles

- **Local-first** — no server, no accounts, no sync
- **Terminal-first** — optimized for tmux/SSH environments
- **Ephemeral review state** — feedback lives for the duration of one review session
- **Git-centric** — branch review, not general file editing
- **Read-only** — not a text editor; never modifies files
- **AI export as a first-class feature** — the output is designed to be fed directly to coding agents
