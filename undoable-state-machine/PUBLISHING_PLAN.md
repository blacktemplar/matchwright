# `undoable-state-machine`: pre-publish plan

## Context

The crate's core algorithm (append-only log, undo-of-undo as redo, rejecting
undos that break a later dependent entry, atomicity on failure) is solid and
well-tested. Before publishing to crates.io, review surfaced three kinds of
gaps. (1) is now done, though not quite as originally planned: `Transition`
is no longer `unsafe`, but instead of keeping the dirty-flag optimization as
a documented "logic error" contract (the original idea, mirroring how
`std::collections::HashMap` documents `Hash`/`Eq`), the trait was split into
a `check_applicable` pre-check (`Result<(), TransitionError>`) and a plain,
safe `apply`; on any `apply` error the state machine now unconditionally
heals by replaying the log instead of relying on a caller-trusted dirty
flag. The former `append` method was also renamed to `apply` as part of this
same change. (2) `undoable-state-machine/DESIGN.md` has already drifted from
the implementation in several places (caller-assigned `TransitionId`, no
`apply`/`undo`/`undo_last` convenience API, merged error type,
generalization over `T: Transition<S>`), and the decision is to retire it
rather than keep reconciling a standalone document that will keep drifting
— the detailed "why" belongs in code doc comments next to what it explains,
with only a short, low-drift summary as a README; (3) the crate is missing
the metadata, docs, license, and CI a crates.io publish needs. With (1)
already done, this plan sequences the rest: retiring the design doc in
favor of in-code docs + a short README first, then remaining code
improvements, then tests, then publishing mechanics. (`tournament-engine`'s
root-level `DESIGN.md` is out of scope — only
`undoable-state-machine/DESIGN.md` is being retired.)

## 1. Retire `DESIGN.md`; move the "why" into code docs + a short README

Files: `undoable-state-machine/DESIGN.md` (deleted),
`undoable-state-machine/README.md` (new),
`undoable-state-machine/src/lib.rs` (expanded doc comments)

- Delete `undoable-state-machine/DESIGN.md`. Its algorithmic content (the
  backward-scan `cancelled`-set computation, the heal-on-error and
  validate-before-commit rationale, the atomicity guarantees, the
  rejected-alternatives reasoning worth keeping) moves to `///` doc
  comments on the specific items they explain — `Transition::check_applicable`,
  `Transition::apply`, `StateMachine::apply`, `StateMachine::build`, and the
  private `replay` helper — so the explanation lives next to the code it
  describes and can't silently drift the way a standalone document did.
- Write a new, deliberately short `README.md`: what the crate does, the
  core append-only-log-plus-undo-entry idea in a paragraph or two, and a
  minimal usage example (mirrors the doctest from §3). No exhaustive
  rationale or alternatives-considered section — that's exactly the part
  that went stale in `DESIGN.md` and doesn't need duplicating now that it
  lives in code docs.
- Add `#![doc = include_str!("../README.md")]` at the top of `lib.rs` so
  the README *is* the crate-level rustdoc rather than a second copy that
  can drift from it — the same single-source-of-truth concern that
  motivated dropping `DESIGN.md` in the first place. This also means §4.3
  (crates.io README) is satisfied by this same file, not a separate step.
- Doc-comment coverage to add as part of this pass (also closes the
  clippy-pedantic gaps noted in §2.2): `Transition` (trait-level,
  documenting the `check_applicable`/`apply` split and `apply`'s
  determinism contract), `TransitionId`, `LogEntry`/`LogEntryKind`,
  `StateMachineError` (per-variant), and every public `StateMachine`
  method (`new`, `current`, `log`, `apply`, `build`).

## 2. Other code improvements

File: `undoable-state-machine/src/lib.rs`

### 2.1 Guard against duplicate `TransitionId`s
`contains_id` and `replay`'s `cancelled` map both silently assume ids are
unique; nothing currently enforces the "must be globally unique" contract
documented on `TransitionId`. Add a check in `apply` (and `build`) that
rejects an incoming entry whose id already appears in the log, via a new
`StateMachineError::DuplicateId(TransitionId)` variant. Back this with a
`known_ids: HashSet<TransitionId>` field on `StateMachine`, kept in sync
with `log` on every successful mutation, so id lookups (`contains_id`,
`Undo` target existence, duplicate rejection) are O(1) instead of a linear
scan — logs can get long enough that scanning them on every `apply` matters.

### 2.2 Clippy pedantic cleanups
- `#[must_use]` on `TransitionId::new` and `StateMachine::new`.
- `Vec::default()` instead of `Default::default()` in `StateMachine::new`.
- Fix the two test `_ => panic!(...)` wildcards to name `StateMachineError::UnknownTarget(_)` explicitly.
- Add `# Errors`/`# Panics` doc sections where clippy pedantic currently flags them missing (`apply`/`build`), as part of the doc pass in §1.

### 2.3 Leave as documented, not code, for now (flagged, not acted on)
- The heal-replay `.expect()` (`lib.rs:137`) stays a panic on a
  determinism-contract violation — panics are memory-safe in Rust (abort/unwind,
  not UB), so this doesn't need to block publishing; call it out in
  `Transition::apply`'s doc as a consequence of the documented determinism
  requirement.
- No explicit `Send + Sync` bound added to `Transition<S>` — leave `T`'s
  bounds to the caller (`Box<dyn Transition<S> + Send + Sync>` when needed);
  note this as a documented option rather than a crate-enforced requirement,
  since forcing it would needlessly constrain single-threaded users.

## 3. Testing additions

File: `undoable-state-machine/src/lib.rs` (`#[cfg(test)] mod tests`)

- A test where an entry's `apply` itself fails during replay triggered by an
  `undo` (not just via `check_applicable` rejecting it, as the existing
  `undo_that_breaks_a_later_entry_is_rejected_and_blames_that_entry` test
  does) — confirming `replay`'s error path is exercised from the `Undo`
  branch of `StateMachine::apply` too, not just from `build`/a direct
  `Apply`.
- A duplicate-id rejection test once §2.1 lands.
- A property-based test (add `proptest` as a dev-dependency) asserting the
  core invariant for random sequences of applies/undos: `current()` always
  equals a from-scratch replay of the log's active set. This is the
  highest-value addition since it's exactly the guarantee the crate exists
  to provide.
- At least one runnable doctest on the crate-level (`//!`) docs showing
  `apply`/`build` end-to-end (the README's existing example already covers
  `apply`/`undo`; extend it, or add a second doctest, to also exercise `build`).

## 4. Crates.io publishing readiness

### 4.1 `undoable-state-machine/Cargo.toml`
Add `description`, `license = "MIT OR Apache-2.0"`, `repository`, `readme`,
`keywords`, `categories`, and `rust-version` (MSRV — edition 2024 needs
1.85+, state it explicitly rather than let consumers discover it from a
build error).

### 4.2 License files
Add `LICENSE-MIT` and `LICENSE-APACHE` at the repo root (workspace-level,
standard Rust dual-license convention).

### 4.3 README
Already covered by §1 — the short README written there, wired in via
`#![doc = include_str!("../README.md")]`, is both the crate-level rustdoc
and what crates.io renders on the package page. No separate step needed.

### 4.4 Repo/CI
- Confirm a git remote/GitHub repo exists (needed for the `repository`
  field and for CI) — flag to the user if one still needs to be created,
  since creating a remote repo is an action outside this plan's scope.
- Add `.github/workflows/ci.yml` running `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, and `cargo doc --no-deps`.

### 4.5 Changelog and final check
- Add `CHANGELOG.md` (keep-a-changelog style), starting at 0.1.0.
- Verify `undoable-state-machine` is available as a crate name on crates.io.
- Run `cargo publish --dry-run` and `cargo package --list` from the crate
  directory to sanity-check the packaged contents before the real publish
  (the actual `cargo publish` is irreversible for a given version and stays
  outside this plan — run it explicitly when ready).

## Verification

- `cargo test` (workspace) — all existing tests plus new ones from §3 pass.
- `cargo clippy --all-targets -- -D warnings` — clean, including pedantic
  fixes from §2.2 (pedantic itself isn't enabled by default, but the
  specific fixes should still land).
- `cargo doc --no-deps --open` — confirm the crate-level docs, the
  `Transition::check_applicable`/`apply` docs, and the doctest all render
  sensibly.
- `cargo publish --dry-run` succeeds from `undoable-state-machine/`.
