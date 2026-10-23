# Tournament Software – Requirements

Oct 23, 2026 · @Ano · Status: Draft v0.2

## 1. Vision and scope

An offline-first, open-source (MIT/Apache-2.0) organizer for head-to-head sports (two sides per match). The engine runs in the browser; a server for sync, publishing and a wider ecosystem comes later. Competitors are online-only, Windows-only or server-only; we aim for one engine for many sports, simple by default with depth on demand.

- **Users:** organizer (runs and corrects the event), referee (enters results, often offline), participants and spectators (read-only, incl. venue screens).
- **Non-goals:** payments, native apps (PWA instead), matches with more than two sides.
- **Terms:** a competition has **phases** (stages); a phase ranks **rankings** (one entrant, or a fixed team of entrants); a **match** is two sides of one or more rankings each (a side formed per match, as in draw-your-partner, is allowed); a **round** is a scheduling slot in which a ranking plays at most once.

## 2. Modes and match structure

Modes: single and double elimination, round robin (with groups), Swiss, random rounds, draw your partner (Monster DYP); later rated leagues. Multi-stage with configurable advancement; byes in every mode; later withdrawals, late entries and disqualifications after the start, each mode documenting what they mean (e.g. walkover in a knockout, annulled or kept results in a round robin, no further pairings in Swiss); configurable scoring, tiebreaks and seeding (manual, by rating, random).

Match structure (open, see section 9): either one generic structure or a plugin axis per phase. v1 needs result-only (chess), single scored game (badminton, soccer) and best of N; result entry adapts to the structure. Later: walkovers/forfeits (a match result, not a bye), team encounters with lineups, two-legged ties, knockout tie-breaks, deeper nesting (tennis), handicaps.

- **Match lifecycle**, each step its own log event: (1) optionally assigned a time and court, (2) started (start time), (3) optionally interim results, (4) final result (finish time).
- **Pairings** are published as soon as a mode makes them. Overrides (swapped or forbidden pairings, sides/colours) are log events that overwrite them. Later: reviewing the pairings of a round before publishing them.
- **Correctness:** Swiss tested against reference engines (bbpPairings, JaVaFo), including colour allocation; each mode documents its rules.
- **Corrections:** an undo is refused only if a later event no longer applies or a match that has started would change; otherwise it applies, and the organizer sees the result and can undo the undo. Later a per-mode policy.

## 3. Configuration and plugins

- Defaults everywhere; each setting is marked common (5–10 per mode) or advanced (collapsed). Presets are saved configurations and can be shared.
- Settings can be changed at any time, also after a phase started: the log is replayed with the new settings, and the change is refused if a later event no longer applies. Like any change, it can be undone.
- Modes, tiebreaks and metrics (and the match structure, if it becomes a plugin) are compile-time Rust trait plugins, vetted by the assembling crate. They are self-describing (stable ID, config schema) and declare what they provide and require, so incompatible combinations are rejected without a list of pairs.
- **Behaviour stability:** a plugin never changes what it computes for an existing configuration, so old logs replay unchanged. A changed behaviour is a new setting (defaulting to the old behaviour) or a new plugin ID.

## 4. Scheduling and operations

v1 runs one self-contained competition per event (court IDs, linkable player IDs).

- **v1:** courts, time slots, breaks, drag-and-drop; check-in before the start.
- **v2:** no-shows, withdrawals and disqualifications during play; auto-scheduling with rest times, referees, player-entered results, web push, public URL, embeds, venue slideshow.
- **Later:** multi-competition tournaments, online registration.

## 5. Data, determinism and sync

- **Event log per competition:** one append-only log for results, configuration and operations (section 2's match lifecycle), with global IDs; undo and corrections are log entries themselves. Events record actor and device for auditing; undo follows the session, not the actor. Events are versioned where a format change is not backwards compatible (additive changes need no version).
- **Personal data** (names, contact, ...) is not in the log: the log holds only IDs, personal data lives in a separate store keyed by them, which can be deleted without breaking replay.
- **Determinism:** seed in the log; times only as event data, never read from the clock during replay; stable ordering (IndexMap); exact scores (integers or rationals, no floats), specified rounding, libm for ratings. Verified by replay, property-based and native/WASM tests plus state-hash checkpoints.
- **Storage safety (v1):** every accepted event is persisted immediately (no explicit save), transactionally (IndexedDB), safe with several open tabs; request persistent storage, warn if refused; automatic backups or export reminders; file export/import anytime. A log that no longer replays after an update stays readable and exportable, next to its last good snapshot.
- **Sync (later):** merging only within one competition (rules to be specified). A tournament is a config document, not a log. Cross-competition court/player clashes are soft rules: warned on any device, fixed by an organizer.

## 6. Frontend and quality

- Organizer "what can be played now" view, undo and redo. Fast offline referee entry. Read-only TV/embed views, print layouts; trees zoomable, lists on phones.
- 500 entrants on a mid-range phone: pairing a round, loading a competition and undoing each < 1 s (snapshot checkpoints if replay is too slow); no accepted result lost; no account needed; WCAG 2.1 AA; translatable; CSV and sport formats (e.g. FIDE TRF). Time zones from v2.

## 7. Ecosystem (future server)

Servers hosting competitions, player databases and ratings across sports; organizations with branded sites.

- **Rules from v1:** competition file is the unit of portability; global IDs; players linkable to external profiles later; results publishable as facts, ratings computed outside competitions; organization profiles (terminology, presets, branding); owner field on every competition.
- **Later:** server topology (multi-tenant, per organization, federation), database per sport or shared, player matching, ownership model.

## 8. Phasing

|  | Scope |
| --- | --- |
| v1 | Offline PWA, one competition per event; round robin, Swiss, single and double elimination, random rounds; result-only, single-game, best-of-N; presets and common options; basic scheduling and check-in; match lifecycle and pairing overrides; standings, print and public view; built-in plugins; data, determinism and storage foundations. |
| v2 | Sync, accounts, roles, referee entry; multi-stage, round robin groups; advanced config; v2 operations (incl. withdrawals); embeds, time zones. |
| Later | Multi-competition tournaments, walkovers/forfeits, late entries, review of pairings before publishing, team encounters, two-legged ties, ratings, third-party plugins, multi-organizer editing, hosted service. |

## 9. Open questions

- First target users (chess, racket sports, esports, school sports?).
- Match structure: one generic structure (match → games → scores, with win conditions) or a plugin axis per phase?
- Merge and conflict rules.
- Lineup rules for team encounters.
- Server topology, ownership.
- Redraw: may an organizer change the seed after a draw (today the seed cannot be undone)?
