---
name: grill-with-docs
description: Mandatory for every RescueMesh change. Before writing any file, read the project docs, grill the user about every design decision the task needs (one question at a time, options, trade-offs and a recommendation), then record each decision verbosely in docs/design-log.md and update docs/architecture.md diagrams and docs/protocol.md in the same change. Use whenever the prompt says "grill with docs" or /grill-with-docs, and whenever a RescueMesh task would change a file.
---

# Grill with docs

RescueMesh is graded on its design and its report. Every decision must be made by a person, with
the reasoning written down. This skill is how an agent works on this repo: read, grill, record, then
build. AGENTS.md ("Design decisions") makes it mandatory.

## 0. Gate

- If the user's prompt does not contain "grill with docs" (or `/grill-with-docs`) and the task would
  create, edit or delete any file, **do not write anything**. Reply in one or two lines: say that
  AGENTS.md requires "grill with docs" for changes and ask them to resend the prompt with it.
- Read-only work (explaining code, answering questions, running builds or tests) does not need it.

## 1. Read before asking

Read these first, every session, so questions are grounded and you do not ask what is already
decided:

1. `AGENTS.md`: owners, architecture rules, git workflow.
2. `docs/design-log.md`: every decision so far, its status, and the open ones.
3. `docs/architecture.md`: components, flows and diagrams, with built/template/planned status.
4. `docs/protocol.md`: messages, encodings, timers.
5. The code the task touches, and its `TODO(<part>)` markers.

Note which part (A–F) the task belongs to and who owns the files. If the task needs a file outside
the user's part or a change to `src/proto.rs`, say so now (AGENTS.md, "Stay in scope").

## 2. Grill

Interview the user until every decision the task needs is settled. Rules:

- **One question at a time** (or one small, tightly related group), using the AskUserQuestion tool
  when it is available. Wait for the answer before the next question; later questions often depend
  on earlier answers.
- **Every question carries a recommendation.** Give 2–4 concrete options with their trade-offs, put
  your recommended option first and say why, citing the design log, the architecture rules, the
  Phase 1 design, papers or measurements where they apply.
- **Challenge the request against the docs.** If the task contradicts a logged decision, an
  architecture rule (determinism, commit-then-reply, idempotent commands, integers in replicated
  data, the fault gate, the serving gate) or another part's interface, raise it as a question
  instead of working around it.
- **Settle the open decisions the task touches, first.** Check the "Open decisions" list under the
  design-log index and the `Open` entries. Most are conflicts between the Phase 1 architecture
  document and the code, left open on purpose for whoever implements the related code. If the task
  touches one, ask the user to decide it before anything else, using the options and recommendation
  in the entry, and update that entry (status, decided by, decision, why) in the same PR.
- **Ask about everything a teammate or the report could ask "why?" about:** messages and fields,
  timer values and defaults, config keys, data structures in replicated state, failure handling,
  retries and timeouts, what gets measured and the event field names, new dependencies, test
  strategy, and choices *not* to do something. Small choices count.
- **Do not decide silently.** If the user says "you choose", state your choice and the reason, and
  record it as decided by the user on your recommendation.
- **Stop grilling** when every decision the implementation needs is answered. Then summarise the
  decisions in a short list and confirm before writing.

## 3. Record

Before or together with the code, in the same branch and PR:

1. **`docs/design-log.md`**: one entry per decision, using the template at the top of that file
   (date, part, status, decided by, context, options considered, decision, why, consequences,
   where, how we check it, report notes). Be verbose: this text feeds the report. Add the entry to
   the index table. If a decision changes an earlier one, add a new entry and mark the old one
   `Amended by Dnn` or `Superseded by Dnn`; never rewrite history. Move open entries to their new
   status once decided.
2. **`docs/architecture.md`**: update every affected section, table and Mermaid diagram (deployment,
   components, sequences, state diagrams, status labels, the "Changes since Phase 1" table). Add a
   new diagram when a new flow appears. Keep Mermaid labels quoted and free of `;`, braces and `<`.
3. **`docs/protocol.md`, `config/local.toml`, `config/lab.example.toml`, README**: when messages,
   timers or config keys change (AGENTS.md, "Keep the docs in sync").

## 4. Build and hand over

- Implement exactly what was decided, in the user's part, following AGENTS.md.
- Run `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings` and `cargo test` (Linux or WSL
  when possible) and report what ran.
- Work on a `part-<letter>-<topic>` or `shared-<topic>` branch, never `main`. In the PR description
  list the design-log IDs the PR adds or changes.
