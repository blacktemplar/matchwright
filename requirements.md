# Tournament Software – Requirements

Oct 23, 2026 · @Ano · Status: Draft v0.1

## 1. Vision and scope

An offline-first, open-source organizer for head-to-head sports (two players or teams per match). The engine runs in the browser; a server for sync, publishing and a wider ecosystem comes later. Competitors are online-only, Windows-only or server-only; we aim for one engine for many sports, simple by default with depth on demand.

- **Users:** organizer (runs and corrects the event), referee (enters results, often offline), participants and spectators (read-only, incl. venue screens).
- **Non-goals:** payments, native apps (PWA instead), matches with more than two sides.

## 2. Modes and match structure

Modes: single and double elimination, round robin (with groups), Swiss, random rounds; later rated leagues. Multi-stage with configurable advancement; byes, withdrawals, late entries and disqualifications everywhere; configurable scoring, tiebreaks and seeding.

Hierarchy: **pairing** (two entrants in a round) → **match** (≥ 1) → **game** (optional) → **score** (optional), relabelled per preset. Covers result-only (chess), single scored game (badminton, soccer), best of N, and later team encounters with lineups. Each level defines a win condition and roll-up; result entry adapts to it. Later: two-legged ties, knockout tie-breaks, deeper nesting (tennis), walkovers, handicaps.

- **Overrides:** swapped or forbidden pairings and sides/colours are explicit log events.
- **Correctness:** Swiss tested against reference engines (bbpPairings, JaVaFo); each preset documents its rules.
- **Corrections:** v1 allows undo only if no later event conflicts (changed future matches must not have started); later a per-mode policy.

## 3. Configuration and plugins

- Three layers with defaults: **preset** → **common options** (5–10) → **advanced** (all, collapsed). Presets can be saved and shared; result-affecting settings lock once a stage starts.
- Pairing modes, tiebreaks, metrics and later match modes are compile-time Rust trait plugins, vetted by the assembling crate. They are self-describing (ID, version, config schema), receive derived state, return results or proposed events, and are validated by the engine. Interfaces use owned, serializable data (WASM-ready).
- Visualization: generic data → standard views and entry widgets → rare custom renderer with fallback.

## 4. Scheduling and operations

v1 runs one self-contained competition per event (court IDs, linkable player IDs).

- **v1:** courts, time slots, breaks, drag-and-drop; check-in and no-shows.
- **v2:** auto-scheduling with rest times, referees, player-entered results, web push, public URL, embeds, venue slideshow.
- **Later:** multi-competition tournaments, online registration.

## 5. Data, determinism and sync

- **Event log per competition:** append-only, versioned events (upcast on replay) with actor, device and global IDs. Personal data in a separate deletable store.
- **Determinism:** seed in the log, no clock, stable ordering (IndexMap), plugin versions in the log, exact scores, specified rounding, libm for ratings. Verified by replay, property-based and native/WASM tests plus state-hash checkpoints.
- **Storage safety (v1):** request persistent storage, warn if refused, automatic backups or export reminders; file export/import anytime.
- **Sync (later):** merging only within one competition (rules to be specified). A tournament is a config document, not a log. Cross-competition court/player clashes are soft rules: warned on any device, fixed by an organizer.

## 6. Frontend and quality

- Organizer "what can be played now" view, undo with preview, review of pairings before publishing. Fast offline referee entry. Read-only TV/embed views, print layouts; trees zoomable, lists on phones.
- 500-entrant pairings < 1 s on a mid-range phone; no accepted result lost; no account needed; WCAG 2.1 AA; translatable; time zones; CSV and sport formats (e.g. FIDE TRF).

## 7. Ecosystem (future server)

Servers hosting competitions, player databases and ratings across sports; organizations with branded sites.

- **Rules from v1:** competition file is the unit of portability; global IDs; players linkable to external profiles later; results publishable as facts, ratings computed outside competitions; organization profiles (terminology, presets, branding); owner field on every competition.
- **Later:** server topology (multi-tenant, per organization, federation), database per sport or shared, player matching, ownership model.

## 8. Phasing

|  | Scope |
| --- | --- |
| v1 | Offline PWA, one competition per event; round robin, Swiss, single elimination; result-only, single-game, best-of-N; presets and common options; basic scheduling and check-in; match control, standings, print and public view; built-in plugins; data, determinism and storage foundations. |
| v2 | Sync, accounts, roles, referee entry; double elimination, random rounds, multi-stage; advanced config; v2 operations. |
| Later | Multi-competition tournaments, team encounters, two-legged ties, ratings, third-party plugins, multi-organizer editing, hosted service. |

## 9. Open questions

- First target users (chess, racket sports, esports, school sports?).
- Merge and conflict rules.
- Lineup rules for team encounters.
- Server topology, ownership, licence.
