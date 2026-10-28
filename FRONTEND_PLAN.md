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

### 4. Storage in the browser (done 2026-10-24)

The crate `tournament-browser` (IndexedDB through `idb`, Web Locks, `BroadcastChannel`, persistence,
download), tested in headless Chrome by the `wasm-test` hook; see DESIGN.md, *Storage*,
*Implementation*. The page starts with the list of competitions (open, export, new, import); the one
open is named after the `#` of the URL. A tab without the lock shows the competition read-only,
catches up with each save of the writer by appending the entries stored after its own log
(`Session::catch_up`, from a position `Storage::read` now takes, which a server sync will need too)
and takes over when it leaves. Export and import use the CLI's file plus the
name (`tournament-app`'s `file`), and a test of the CLI opens what the app exports and vice versa.
Changed on the way: one `Header` for every screen, since an `h1` replaced by the next screen made
the e2e tests' elements stale (`Page::expect_texts` now looks again if one is); the harness got tabs
and a download directory per test; persistence is asked for when a competition is opened, which
headless Chrome refuses, so its warning is on the page in every e2e test. Not done: export reminders
or automatic backups (`requirements.md`), a warning before leaving with changes not yet written,
handling a newer version of the database while another tab has an older one open
(`versionchange`), renaming and deleting competitions; the "last good snapshot" next to a log that
no longer replays waits for snapshots.

IndexedDB behind the storage trait (pick the crate here, e.g. `indexed_db_futures` or `rexie`):
one record per log entry, names apart; the single writer tab via Web Locks, followers via
BroadcastChannel; asking for persistent storage and warning if refused; export to and import from
a file; a list of competitions to open. A log that no longer replays stays exportable.

### 5. Configuring a phase (done 2026-10-25)

The form is data of `tournament-app` (`form`: `PhaseForm`, its `FormView`, changes by `Path`),
laid out by `tournament-gui`'s `phase.rs`; see DESIGN.md, *Configuring a phase*. The phase once
added shows as a summary (which step 2 lacked), edited in a dialog before and after the start.
Changed on the way: a form shows every field at once, so a field still answered as offered follows
a change of a field before it in schema order (another format brings its ranking and metrics),
never one after it (removing a metric would otherwise drop the criterion needing it unnoticed;
an issue shows it instead); the implied plugins are shown in
their list, not chosen; issues are translated by kind, with plugin labels as variables
(`Arg::Plugin`), and the fields of a phase itself by the message `phase` in `plugins.ftl`. The
components are copied into `src/components/` and changed there (trigger named by its label,
contrast), since `css_module` hashes their class names. Not done: translating what a plugin or an
incompatibility says inside an issue (a rejection's reason, a fact's name, which for a ranking
criterion is a Rust type name, shown only when no plugin provides the fact), choosing a plugin by keyboard is the select component's (not checked beyond axe), and the
dialog has no visible close button (Escape and Cancel close it).

A form generated from the phase draft's schema, the GUI counterpart of the CLI's questionnaire:
fields by kind with their ranges, plugin choices per axis, implied plugins, issues shown where they
arise; editing the phase after the start (`reconfigure_phase`). First uses of Dioxus components
(select, dialog).

### 6. Running a competition (done 2026-10-25)

See DESIGN.md, *Running a competition*. Most of the screen existed since step 2 (rounds, reopening,
undo/redo, standings, stages); added: the matches playable now (`Session::playable`) as a list next
to the rounds, a result typed in as the match's number and `1`/`X`/`2` (`Session::report_entered`),
the focus moving on after a result button, and announcements in a live region of what was reported,
reopened, undone or redone (`Session::undo`/`redo` return the `Action`, the session keeping stacks
of them instead of counts; `Reported`/`Reopened` carry the match's row). Changed on the way: stages
are translated, since `PhaseFormat::stage` returns a `Stage` (key, numbers, English) translated as
`.stage-<key>` of the format's message. Not done: pagination of a long round (one round at a time
is the only limit, fine for the first use cases), filtering the lists by entrant, a result entry
that adapts to a match structure (best of N, scores), which waits for match structures
(`requirements.md`), and `SessionHistory::can_undo`/`can_redo` in `undoable-state-machine` (the
session's stacks of actions are needed anyway).

The organizer's main screen: matches one round at a time (or paginated), fast result entry by
keyboard, reopening a result, undo/redo with an announcement of what happened (`aria-live`),
standings with the metric columns, stages for elimination formats, "what can be played now".

### 7. Offline and release (done 2026-10-25, not released)

See DESIGN.md, *Offline and installing*. The page is a PWA: `tournament-gui/public/` holds the
manifest, icons (SVG, and PNGs rendered from it), the service worker `sw.js` (assets from the cache
once kept, everything else from the network and from the cache when that fails; a changed page
drops the old assets) and `pwa.js`, which registers it, hands it what the first visit loaded before
it took over, and keeps Chromium's install offer. `dx` already runs a current `wasm-opt`; the
release build is `dx build -p tournament-gui --release --debug-symbols false` (2.4 MB instead of
4.4 MB), on which the e2e tests run too. The warning of a refused persistent storage says both
causes, offers Export (on a competition's screen), Install (while the browser offers it, asking for
persistence again once installed) and "Don't show again" (local storage). The harness can take the
server away (`Page::go_offline`), check what is kept (`expect_kept_offline`) and ask Chrome's
DevTools whether the page is installable (`expect_installable`; headless Chrome makes the offer
too). Changed on the way: hosting is recommended (GitHub Pages, built by CI with `--base-path`,
since the wasm holds local paths of the assets) but not set up, as nothing is released yet. Not
done: the export reminder ("changed since its last export"), a timeout for the network on a bad
connection (the page waits for it before falling back to the cache), telling the operator that a
new version is loaded, a test of a version replacing another, and translating the manifest's name.

PWA: web app manifest, service worker caching the page, release build with `wasm-opt` (trunk's
default `wasm-opt` was too old for current Rust; check `dx`'s), where to host it.

Rework the warning shown when the browser refuses persistent storage (step 4), which Chrome does
for a site that is not installed or used much, and Firefox always does in a private window (whose
storage is deleted with the session, which the page cannot detect). Since no button on the page
can change the answer, it is a permanent nag now: word it for both cases ("may not keep these
competitions, e.g. in a private window or when short of space"), give it an Export button, make it
dismissible, offer installing the app (after which Chrome usually grants it) and ask again after
an install. Possibly the export reminder of `requirements.md` ("changed since its last export").

### 8. Styling before the release

Modern, easy to use and neutral in taste; for laptops and tablets, and a phone alone for small
events; 20–40 entrants typically. In parts:

1. **Layout** (done 2026-10-25), see DESIGN.md, *Layout*: one bar on top, cards in columns where
   there is room (buttons choosing the column on a narrow screen), a match as a card of two rows
   whose buttons say only "Wins" below each name, names never cut, rounds stepped through by a
   select, the standings scrolling sideways, the language in the footer. The e2e tests run at
   1280×900. The list of competitions says how far each is (added 2026-10-26: not started, the
   round being played and its matches to play, or every match played), kept by the storage with
   the listing.
2. **Palette and details** (done 2026-10-26), see DESIGN.md, *Layout*: colour tokens in light and
   dark following the system or chosen in the footer, the accent the navy of the icon, the components' theme mapped onto
   them, hover and chosen states, a finished match receding, an empty list of entrants saying so,
   motion respecting `prefers-reduced-motion`, a more compact storage warning. Checked by axe-core
   in both schemes once, by hand; the e2e tests run in light only.
3. **Finding an entrant** (done 2026-10-26, removed 2026-10-28), see DESIGN.md, *Running a
   competition*: a field above the matches showing only those of entrants whose name contains what
   is typed. Removed in favour of the browser's find in page, since it overlapped the typed entry
   of results.
4. **The bracket** (done 2026-10-26), see DESIGN.md, *The bracket*: a single and a double
   elimination show every pairing from the start, as columns of stages joined by lines, in place of
   the columns on the full width, chosen by the view switch on top, which a knockout shows at every
   width ("Matches & standings" | "Bracket", on a narrow screen a third button). Reporting from the
   bracket is not done; results are reported in the list.

### Later, not planned in detail

Reporting results from the bracket, print layouts, read-only public/TV view, scheduling
with drag and drop (the components' drag-and-drop list), check-in, CSV and FIDE TRF, snapshots for
large events, the CLI on top of the app crate, and what becomes of `tournament-web`.

## Open questions

- ~~Is the storage trait async, and does the CLI share it? (step 1)~~ Async; the CLI's file storage
  fits it with futures that are ready at once, once the CLI sits on the app crate.
- ~~One e2e crate for both pages, or one per page? (step 2)~~ One per page, on a shared
  `e2e-harness`.
- ~~CSS: plain CSS files, or something like Tailwind (an extra tool)? (step 2)~~ Plain CSS.
- Hosting of the page. (step 7) Any static host with HTTPS; GitHub Pages deployed by CI is
  recommended (DESIGN.md, *Hosting*), to be decided when it is released.
