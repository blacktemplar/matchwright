# AGENTS.md

This file provides guidance to AI agents when working with code in this repository.

## Commands

Verify changes by running the hooks from `.pre-commit-config.yaml` (CI runs the same ones) rather than the individual cargo commands:

```sh
prek run --all-files          # or: pre-commit run --all-files
prek run --all-files cargo-test   # a single hook
```

The hooks are `cargo-fmt`, `cargo-clippy` (`-D warnings`), `cargo-test`, `cargo-doc` and `cargo-sort`. `cargo-fmt` needs **nightly** rustfmt because `rustfmt.toml` uses unstable options.

- Run a single test: `cargo test -p tournament-engine <test_name_substring>`. Integration tests live in `tournament-engine/tests/` (pick a file with `--test full_tournament`).
- Doc tests count; several public items carry runnable examples.
- Benchmark: `cargo bench -p tournament-engine --bench large_tournament` (criterion).
- Example: `cargo run -p tournament-engine --example run_tournament`.
- CLI: `cargo run -p tournament-cli -- [FILE]` (see `tournament-cli/README.md` for commands and presets).
- Profile `release-checked`: release with debug assertions and overflow checks.
- Keep `Cargo.toml` dependency lists sorted (the `cargo-sort` hook checks this). Dependency versions are declared only in the root `[workspace.dependencies]`; members use `dep.workspace = true`. `plugin-factory` and its proc-macro crate `plugin-factory-derive` are the exception on purpose: they declare everything themselves, including `version`/`edition`/`license`, so that they can be moved out of the workspace together. Don't give them workspace references, and don't let them depend on the other crates.
- `undoable-state-machine` is a git dependency (same author). To develop against a local checkout at `../undoable-state-machine`, uncomment the `[patch]` in `.cargo/config.toml`.

## Architecture

Two crates: `tournament-engine` (the library) and `tournament-cli` (an interactive REPL on top of it). `DESIGN.md` is the authoritative design record, with the rationale and rejected alternatives for each decision. Read the relevant section before changing a mechanism. Sections marked *planned* are not implemented. Some type names in it may lag behind recent renames (e.g. ranking "criteria").

### Global constraints (from DESIGN.md, apply everywhere)

- **State changes only through logged transitions.** Progress is an `undoable-state-machine` `StateMachine<Competition, Box<dyn CompetitionTransition>>` (`CompetitionStateMachine`). Undo means replaying the remaining log from the initial state. `Competition`'s mutators are crate-internal on purpose.
- **Determinism under replay.** `Transition::apply` must be a pure function of state plus the transition's own data. Randomness comes from the competition seed (`SetCompetitionSeed`, `Competition::new_seeded_rng`). Ids (`MatchId`, `RankingId`) are UUIDv5s derived from content. Anything non-derivable (names, timestamps, external ids) must arrive as transition data.
- **Runtime-pluggable, third-party-open axes.** Each axis is a public unsealed trait used as `Box<dyn Trait>`: `PhaseFormat`, `Ranker`/`SliceRanker`/`RankingCriterion`, `MetricsCalculator`. All but `RankingCriterion` share the hand-implemented `Plugin` supertrait (`Debug + DynClone` plus `declare`, which states the facts a plugin provides to and requires of its phase), and each has a hand-written `impl Clone for Box<dyn Trait>`. Don't use closed enums, generics on entities, or `enum_dispatch`. DESIGN.md explains why generic entities fail with `typetag`.
- **Plugins register themselves in the catalog** (`plugins::catalog`). An axis declares its registry with `plugin_kind!` next to its trait, and every plugin submits a `PluginEntry` with its id, label, description and, unless it has to be configured, a constructor. `AddPhase::new` fills a required fact nothing provides from the catalog (`PhasePlugins::resolve`), so a new plugin needs an entry and a consumer must not grow a table of plugins. `PROPOSAL-plugin-catalog.md` holds the steps still to come (schemas, builder, the CLI).
- **No untyped data bags.** Open-ended data is keyed by Rust type through `TypeMap` (`type_map.rs`). Metrics are `TypeMapValue`s, identified in-process by `TypeId`. Consumer data (e.g. the CLI's display name) is stored as an annotation in a `TypeMap` field on the entity. It is written by the transition that creates the entity (`Entrant::new(id).with_annotation(..)`).

### Persistence

- Two paths: a `Competition` snapshot, and the transition log (the CLI saves the log as JSON, so undo/redo survive reload). Deserializing the log rebuilds the state by replay. A log that doesn't replay fails to load.
- Trait objects serialize via `typetag` with **adjacent tagging** (`#[typetag::serde(tag = "type", content = "value")]`). Internal tagging breaks non-self-describing formats like postcard, so don't switch to it. Tests exercise both postcard and serde_json.
- **Tags are persistence contracts.** Every `CompetitionTransition` impl has an explicit namespaced tag (e.g. `#[typetag::serde(name = "tournament_engine::...")]`), pinned in tests. New transitions/plugins/annotations need an explicit tag; never change an existing one. (Plugins and metrics still use bare default tags; DESIGN.md flags this as outstanding.)
- `IdMap` (`map.rs`) and `TypeMap` wrap `IndexMap` and serialize as plain sequences in insertion order. Code may rely on that order (e.g. match numbers are derived from insertion index).

### Engine data flow

`Competition` → `Phase` (holding its `PhaseFormat`, ranker, metrics calculators, `matches`, `byes`, `rankings`) → `Match` / `Bye` / `Ranking`. On `StartCompetition` and after every reported result the phase format schedules a `Schedule` of matches and byes (`create_new_matches(&PhaseData)`); a bye is final when scheduled, so each calculator's `update_after_bye` runs right away, followed by a re-rank. On `ReportMatchResult` each `MetricsCalculator` updates metrics incrementally (`update_after_match_result_changed`, which gets the *old* metrics of all calculators so order doesn't matter), then the ranker re-sorts. This is the hot path, so `LexicographicalCriteria` resolves typed metric columns once per sort instead of downcasting per comparison.

### CLI

`tournament-cli/src/lib.rs` holds everything testable. `command.rs` parses a line into a `Command`, and `session.rs` executes it against the state machine and handles save/load (atomic via temp file). `phase.rs` holds `PhaseConfig` and the presets. `wizard.rs` is the interactive `inquire`-based phase configuration. `annotations.rs` defines CLI-only entity data such as display names. `main.rs` only does terminal I/O (rustyline). The engine knows nothing about display.

## Working style

- Prefer tools and commands that are already allowed, plus the normal Read/Edit/Write tools, over anything that triggers a new permission prompt. For example, edit files with the Edit tool rather than `sed`, and run the existing commands as they are rather than wrapping them in new pipelines. Only ask for a new permission when there's no reasonable alternative.

## Conventions

- The user prefers invariants enforced by types/the compiler over runtime checks or self-reported plugin facts, and is cautious about costs on the result-reporting hot path.
- Doc comments follow KISS: say what a public item does and everything a caller needs in order to use it (contract, panics, errors, ordering guarantees), nothing more. No historical rationale, no comparison with rejected alternatives — that belongs in `DESIGN.md`. Where a *maintainer* needs the reasoning, put it in a plain `//` comment next to the code instead of in the rustdoc.
