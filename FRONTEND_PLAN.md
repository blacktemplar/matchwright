# Frontend plan

The plan for building the GUI, one step per session. Mark a step done here when it is finished, and
note what changed in the plan on the way. `requirements.md` says what the frontend must do;
`DESIGN.md` (from step 0 on) records the decisions and their reasons.

## Decisions made (2026-10-23)

- **Framework: Dioxus** (0.7), rendering real DOM in the browser, built with its CLI `dx`.
  Canvas GUIs (egui, iced, Slint) are out: WCAG, print layouts, embeds and phones need real HTML.
  Leptos was the close alternative; both were spiked in `spikes/` (same app on a shared core,
  measurements in `spikes/README.md`). Dioxus won on how its code reads, on updating long lists
  built from plain rows (1.5–2x faster than Leptos), and on keeping desktop/mobile possible later.
- **A UI-free app crate between engine and GUI.** The engine stays as it is: what is true about a
  competition whoever looks at it, and replays identically. The app crate holds what is about an
  operator at a screen: the session (state machine, its undo history), turning an action into
  transitions (e.g. a new entrant: id, `AddEntrant`, name), checks only a UI cares about, names
  (`People`), saving and loading through a storage trait, data for screens (rows, standings,
  possible actions), and messages for the operator. It is the CLI's `Session` with typed data
  instead of text; later the CLI can sit on top of it.
- **Storage: IndexedDB, one record per log entry**, appended in a transaction as each change is
  accepted. One writer tab per competition (Web Locks API); other tabs open it read-only and follow
  the writer (BroadcastChannel). Names are stored apart from the log.
- **Translation: Fluent** (`fluent-bundle`) for all UI text from the first screen. Plugin labels,
  descriptions and fields get keys derived from the plugin id and field name, with the English label
  from the code as fallback, so the engine needs no change.
- **Accessibility (WCAG 2.1 AA):** the Dioxus team's components (`dx components add` copies their
  source into the project; they build on `dioxus-primitives` from `DioxusLabs/dioxus-components`,
  unreleased, so a git dependency pinned in `Cargo.lock`) where a widget needs them, semantic HTML
  everywhere else, and an axe-core check in the e2e tests. Switching to own components later is
  possible one at a time.
- **Performance:** fine for the first use cases (20–50 entrants). With 1000 entrants, undo, redo
  and loading replay the whole log (~1.8 s in wasm at 5000 results, several seconds on a phone);
  snapshot checkpoints fix that and come later. Independently of that, the UI shows one round (or a
  page) of matches at a time rather than thousands of rows.

## Steps

### 0. Record the decisions (done 2026-10-23)

DESIGN.md has the section `Frontend`, with the measurements copied from `spikes/README.md` so that
they outlive the spikes.

- Add a `Frontend` section to `DESIGN.md` with the decisions above, their alternatives (Leptos,
  canvas GUIs, whole-log storage, several writer tabs, own components) and the measurements.
- Commit `spikes/` as the record of the comparison; it is deleted once step 2 has a GUI to measure.

Done when DESIGN.md has the section and the spikes are committed.

### 1. The app crate (done 2026-10-23)

`tournament-app`. The storage trait is async and takes records (one per log entry and per person);
the session changes in memory and a save writes what was appended since the last one, see
DESIGN.md, *Storage*. Changed on the way: `People` moved from the CLI into the app crate (the CLI
re-exports it); the CLI keeps its own session and storage until it sits on the app crate. The
session counts what can be undone and redone itself, which `undoable-state-machine` could tell
instead (`SessionHistory::can_undo`/`can_redo`), a small change there worth making. Undo and redo
do not say yet what they undid, which step 6 needs for its announcement.

A new crate (name to choose, e.g. `tournament-app`), in the workspace, building for wasm, with no UI
dependency. Start from `spikes/spike-core` and the CLI's `Session`:

- creating and opening a competition, entrants with names, a phase from a draft, start, reporting
  and undoing results, session undo/redo;
- data for screens: matches (by round, with stages and names), standings with the metric columns,
  what can be done now;
- typed errors that the GUI turns into messages (later Fluent keys);
- a storage trait. IndexedDB is asynchronous, so decide here whether the trait is async (likely)
  and how the CLI's synchronous file storage fits; an in-memory implementation for tests.

Tests native and in wasm, as for the other crates. Update AGENTS.md (crates, commands).

Done when the crate covers what the spike's core does, with tests, and the hooks pass.

### 2. The GUI crate, first screen (done 2026-10-24)

`tournament-gui`, built with `dx build -p tournament-gui` (dx 0.7.10, as `dioxus` in `Cargo.lock`).
The e2e harness became the crate `e2e-harness`, used by `tournament-web-e2e` and the new
`tournament-gui-e2e`, each with its own manual hook (`web-e2e`, `gui-e2e`) and CI job; every e2e
test of both pages ends with an axe-core check (WCAG 2.1 A/AA), axe-core downloaded at a pinned
version and cached. CSS is a plain file. Changed on the way: the e2e tests run on the release
build, since a debug build tries to reach `dx serve`; sides of a match come from the random seed, so
the tests do not depend on them; `plugin-factory-derive` boxes an `Expr`, since Dioxus enables
features of `syn` that made a clippy lint fire. The screen does not name the phase's format once
added (the session does not tell it yet). Not measured with the GUI yet; the spikes' numbers stay in DESIGN.md.

A Dioxus crate (e.g. `tournament-gui`) on the app crate, with in-memory storage for now:

- `dx` setup (`Dioxus.toml`, assets, CSS via `asset!()`), workspace dependencies, the wasm clippy
  hook extended, how CI builds it;
- one screen like the spike's: entrants, a phase with a format's defaults, start, matches of one
  round, results, undo/redo, standings;
- e2e tests: reuse the WebDriver harness of `tournament-web-e2e` (extract what both need, or a
  second crate), with an axe-core check on every page;
- delete `spikes/`.

Done when the screen works, its e2e tests (with axe) pass in CI, and AGENTS.md describes the crate.

### 3. Translation (done 2026-10-24)

English and German, see DESIGN.md, *Translation*. The machinery is in `tournament-app` (`i18n`:
`Language`, `Translator`, `check` for a consumer's tests); errors are a `Message` saying a `Text`
(key and variables), so an error shown is translated again when the language changes. The GUI
has its own messages, `t!` and `tr.get`, a language list in the header (the choice kept in local
storage, else the browser's languages), and `<html lang>`; Chrome in the e2e tests prefers English.
Changed on the way: plugin keys are attributes of one message per plugin (`plugin-<id>` with
`.label`, `.field-<name>`, ...), plus `.column` for the header of a calculator's column, which the
standings now name by the calculator's id (`plugin_factory::id_of`). Not translated yet: stages
(still English from `PhaseFormat::stage`; step 6), issues of a draft (step 5), the engine's and
storage's reasons inside an error, and number formats (`1.5`). The fields of `PhasePlugins` itself
(format, ranking, metrics) are no plugin, so they need keys of their own in step 5.

Fluent bundles (English first, plus one more language to prove it), a small helper for components,
keys for plugin labels/descriptions/fields with the code's label as fallback, `<html lang>` set,
choosing the language. All text of step 2 goes through it.

### 4. Storage in the browser

IndexedDB behind the storage trait (pick the crate here, e.g. `indexed_db_futures` or `rexie`):
one record per log entry, names apart; the single writer tab via Web Locks, followers via
BroadcastChannel; asking for persistent storage and warning if refused; export to and import from
a file; a list of competitions to open. A log that no longer replays stays exportable.

### 5. Configuring a phase

A form generated from the phase draft's schema, the GUI counterpart of the CLI's questionnaire:
fields by kind with their ranges, plugin choices per axis, implied plugins, issues shown where they
arise; editing the phase after the start (`reconfigure_phase`). First uses of Dioxus components
(select, dialog).

### 6. Running a competition

The organizer's main screen: matches one round at a time (or paginated), fast result entry by
keyboard, reopening a result, undo/redo with an announcement of what happened (`aria-live`),
standings with the metric columns, stages for elimination formats, "what can be played now".

### 7. Offline and release

PWA: web app manifest, service worker caching the page, release build with `wasm-opt` (trunk's
default `wasm-opt` was too old for current Rust; check `dx`'s), where to host it.

### Later, not planned in detail

Bracket view (SVG with a text alternative), print layouts, read-only public/TV view, scheduling
with drag and drop (the components' drag-and-drop list), check-in, CSV and FIDE TRF, snapshots for
large events, the CLI on top of the app crate, and what becomes of `tournament-web`.

## Open questions

- ~~Is the storage trait async, and does the CLI share it? (step 1)~~ Async; the CLI's file storage
  fits it with futures that are ready at once, once the CLI sits on the app crate.
- ~~One e2e crate for both pages, or one per page? (step 2)~~ One per page, on a shared
  `e2e-harness`.
- ~~CSS: plain CSS files, or something like Tailwind (an extra tool)? (step 2)~~ Plain CSS.
- Hosting of the page. (step 7)
