---
name: mvp-step
description: Implement one step of the jterm MVP from SPEC.md §5 end to end — read the step, research crate APIs, implement, verify every acceptance criterion, update docs, and commit as mvp(step-N). Use when the user says "start step N", "do the next step", or "/mvp-step N".
argument-hint: "[step number]"
---

# Implement MVP step $ARGUMENTS

If no number was given, take the next step from the SessionStart context (or from
`git log --oneline | grep 'mvp(step-'`).

## 1. Load the step

- Read SPEC.md §5 "Step $ARGUMENTS" in full, plus every § it references (e.g. §4.3 for input).
- Read CLAUDE.md "Project state" and "Notes" — they record decisions that override SPEC.
- Confirm the previous step's commit exists. If the working tree is dirty, ask before continuing.
- If the step can't be done as written (crate API differs, Windows-only blocker, spec conflict),
  **stop and explain** with a proposed alternative. Don't deviate silently (SPEC rule 6).
  Known open question: Step 4 shell spawning on Windows (`$SHELL` doesn't exist) — ask the user.

## 2. Research before coding

Crate versions in this repo are newer than model training data. Never write against a
remembered API:

- Delegate to the `crate-researcher` agent for anything non-trivial in `alacritty_terminal` or
  `egui`/`eframe` (e.g. "how does `alacritty_terminal::tty::new` take its options in 0.26?").
- Or look yourself: sources are in `~/.cargo/registry/src/*/<crate>-<version>/`; check
  `Cargo.lock` for the exact version. context7 MCP has docs for egui and alacritty.
- `egui_term` (github.com/Harzu/egui_term) is the reference for rendering/input. It may lag our
  egui version: adapt, don't copy blindly.
- New dependency: `cargo search <crate>` for the latest version, check its MSRV against
  `rust-version` in Cargo.toml, then `cargo add`.

## 3. Implement

- Follow the module layout in SPEC §3.3 and the conventions in CLAUDE.md (no `unwrap()` without a
  documented invariant, short `Term` lock scope, repaint only on events, doc comments say *why*).
- Write the unit tests the step's **Accept** line asks for. Prefer table-driven tests.
- Rust files are auto-formatted after every Edit/Write by a hook; don't hand-format.

## 4. Verify — every acceptance criterion

Use the `verify` skill. For each criterion in the step's **Accept** line, record how it was
verified: unit test name, smoke-test output, screenshot, or "needs manual check by user"
(e.g. interactive vim). Don't claim a criterion passes without evidence.

For a second opinion before committing, run the `acceptance-checker` agent on the step and the
`rust-reviewer` agent on the diff, and fix what they find.

## 5. Update docs

- CLAUDE.md "Project state": which steps are done and what the app does now.
- CLAUDE.md "Architecture"/"Notes": any non-obvious decision, gotcha, or deviation from SPEC.
- SPEC.md: only if the step changed the plan (e.g. a new file in the §3.3 layout); keep edits
  minimal and say so in the commit.
- Edit docs with the Edit tool. Don't use sed/perl for text containing backslashes (Windows
  paths) — it silently corrupts them.

## 6. Commit

Commit directly on `main`:

```
mvp(step-N): <summary>

- bullet per notable change
```

A PreToolUse hook runs fmt/clippy/tests before every commit and blocks it on failure — fix the
cause, never bypass it.

## 7. Report

Tell the user: what was built, the acceptance table (criterion → evidence), anything left for
manual checking, decisions made on their behalf, and what the next step will need from them.
