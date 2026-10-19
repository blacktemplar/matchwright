# AGENTS.md

This file provides guidance to AI agents when working with code in this repository.

> **Edit files only with the Edit/Write tools.** Never change files through shell scripts (`python`, `sed`, `awk`, heredocs, ...), not even for large or repetitive changes: split them into several Edit calls instead. See *Working style*.

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
- `undoable-state-machine` is a git dependency (same author) that follows its `main`; `Cargo.lock` pins the commit. After pushing a change there, pick it up with `cargo update -p undoable-state-machine`.

## Architecture

Two crates: `tournament-engine` (the library) and `tournament-cli` (an interactive REPL on top of it), plus `plugin-factory` (generic plugin axes, catalog, schemas, drafts, compatibility), which the engine builds on and which must not depend on the others. `DESIGN.md` is the authoritative design record, with the rationale and rejected alternatives for each decision. Read the relevant section before changing a mechanism. Sections marked *planned* are not implemented. Some type names in it may lag behind recent renames (e.g. ranking "criteria").

### Global constraints (from DESIGN.md, apply everywhere)

- **State changes only through logged transitions.** Progress is an `undoable-state-machine` `StateMachine<Competition, Box<dyn CompetitionTransition>>` (`CompetitionStateMachine`). Undo means replaying the remaining log from the initial state. `Competition`'s mutators are crate-internal on purpose.
- **Determinism under replay.** `Transition::apply` must be a pure function of state plus the transition's own data. Randomness comes from the competition seed (`SetCompetitionSeed`, `Competition::new_seeded_rng`). Ids (`MatchId`, `RankingId`) are UUIDv5s derived from content. Anything non-derivable (names, timestamps, external ids) must arrive as transition data.
- **Runtime-pluggable, third-party-open axes, built on `plugin-factory`.** Each axis is a public unsealed trait used as `Box<dyn Trait>`, declared with `#[plugin_factory::axis]` with `plugin_factory::Plugin + DynClone` as supertraits: `PhaseFormat`, `SliceRanker`, `MetricsCalculator`. Each has a hand-written `impl Clone for Box<dyn Trait>`. A plugin derives `Plugin` (`#[plugin(id, label, description, declare_static, declare)]`; its schema comes from its fields, each needing `#[plugin(label = ..)]`) and registers with `#[register]` on its axis impl, which also tags it for `typetag` with its id. Facts a plugin provides/requires go through `plugin_factory::compatibility::Declaration`; the engine's own facts are in `plugins::facts`. Runtime state of a plugin (e.g. the bracket) is a `#[serde(default)] #[plugin(skip)]` field. The ranker is not an axis: a phase is always ranked by a `HierarchicalRanker` built from its list of slice rankers. A criterion like `MorePoints` is a `SliceRanker` implementing the helper trait `RankingCriterion` (metric type + comparison), declares `declare_static = requires_metric::<Self>` and calls `rank_by`. Don't use closed enums, generics on entities, or `enum_dispatch`. DESIGN.md explains why generic entities fail with `typetag`.
- **A phase's plugins are the composite `PhasePlugins`** (`Configurable`: format, ranking, metrics). Facts nothing provides are settled before the transition, by `plugin_factory::draft::finish` (or `PhasePlugins::settle`) with `Competition::declare_phase_context` as the context; only fieldless plugins are added. Its calculators are a `plugin_factory::schema::DependencyOrderedSet`, which keeps plugins in running order on `DependencyOrderedSet::new` and on deserialization and rejects duplicates and cycles (`DependencyOrderedSetError`); the slice rankers are an ordered `Vec`. `AddPhase::check_applicable` runs `compatibility::check`. A new plugin needs only its derive and `#[register]`; a consumer must not grow a table of plugins.
- **No untyped data bags.** Open-ended data is keyed by Rust type through `TypeMap` (`type_map.rs`). Metrics are `TypeMapValue`s, identified in-process by `TypeId`. Consumer data (e.g. the CLI's display name) is stored as an annotation in a `TypeMap` field on the entity. It is written by the transition that creates the entity (`Entrant::new(id).with_annotation(..)`).

### Persistence

- Two paths: a `Competition` snapshot, and the transition log (the CLI saves the log as JSON, so undo/redo survive reload). Deserializing the log rebuilds the state by replay. A log that doesn't replay fails to load.
- Trait objects serialize via `typetag` with **adjacent tagging** (`#[typetag::serde(tag = "type", content = "value")]`). Internal tagging breaks non-self-describing formats like postcard, so don't switch to it. Tests exercise both postcard and serde_json.
- **Tags are persistence contracts.** Every `CompetitionTransition` impl has an explicit namespaced tag (e.g. `#[typetag::serde(name = "tournament_engine::...")]`), pinned in tests. A plugin's tag is its `PluginType::ID` (namespaced, e.g. `tournament_engine::points`), pinned in `tests/transition_log_serialization.rs`. New transitions/plugins/annotations need an explicit tag; never change an existing one. (Most metrics still use bare default tags; DESIGN.md flags this as outstanding.)
- `IdMap` (`map.rs`) and `TypeMap` wrap `IndexMap` and serialize as plain sequences in insertion order. Code may rely on that order (e.g. match numbers are derived from insertion index).

### Engine data flow

`Competition` → `Phase` (holding its `PhaseFormat`, ranker, metrics calculators, `matches`, `byes`, `rankings`) → `Match` / `Bye` / `Ranking`. On `StartCompetition` and after every reported result the phase format schedules a `Schedule` of matches and byes (`create_new_matches(&PhaseData)`); a bye is final when scheduled, so each calculator's `update_after_bye` runs right away, followed by a re-rank. On `ReportMatchResult` each `MetricsCalculator` updates metrics incrementally (`update_after_match_result_changed`, which gets the *old* metrics of all calculators so order doesn't matter), then the ranker re-sorts. This is the hot path, so `rank_by` looks up each ranking's metric once per sort instead of per comparison.

### CLI

`tournament-cli/src/lib.rs` holds everything testable. `command.rs` parses a line into a `Command`, and `session.rs` executes it against the state machine and handles save/load (atomic via temp file). `questionnaire.rs` is the generic, schema-driven questionnaire: it fills a `Draft` of any `Configurable` through a `Prompter` (`Terminal` asks via `inquire`, `AcceptDefaults` takes every offered answer) and knows no plugin. `wizard.rs` applies it to `PhasePlugins`. A preset (`command.rs`) is only a phase format; everything else is what the format `recommends`, so the CLI keeps no table of plugins. `annotations.rs` defines CLI-only entity data such as display names. `main.rs` only does terminal I/O (rustyline). The engine knows nothing about display.

## Working style

- Prefer tools and commands that are already allowed, plus the normal Read/Edit/Write tools, over anything that triggers a new permission prompt. For example, edit files with the Edit tool rather than `sed` or a `python` script, and run the existing commands as they are rather than wrapping them in new pipelines. Only ask for a new permission when there's no reasonable alternative.

## Conventions

- The user prefers invariants enforced by types/the compiler over runtime checks or self-reported plugin facts, and is cautious about costs on the result-reporting hot path.
- Plugin labels and descriptions follow the guidelines in the docs of `tournament_engine::plugins` ("Labelling a plugin"), checked in part by a test there. The answer a field offers is `#[plugin(default ..)]`; `#[serde(default)]` only governs loading and offers nothing.
- Doc comments follow KISS: say what a public item does and everything a caller needs in order to use it (contract, panics, errors, ordering guarantees), nothing more. No historical rationale, no comparison with rejected alternatives — that belongs in `DESIGN.md`. Where a *maintainer* needs the reasoning, put it in a plain `//` comment next to the code instead of in the rustdoc.
