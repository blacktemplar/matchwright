# GUI spikes: Leptos vs. Dioxus

Throwaway prototypes for choosing the GUI framework. Both apps do the same thing on the same
`spike-core` (a `Tournament` wrapping the engine's state machine, session history and names, handing
out plain rows): add entrants (or 8 demo ones), pick a format and add the phase, start, report
results per match, reopen a result, undo and redo, and show the standings.

It is a workspace of its own, so that the frameworks stay out of the main workspace and its hooks.

```sh
cd spikes/spike-leptos && trunk serve --port 8081
cd spikes/spike-dioxus && trunk serve --port 8082
```

Dioxus is meant to be developed with its own CLI, which adds hot-patching of Rust code (edits show
up without reloading the page or losing the state). To try that, install the CLI matching the
`dioxus` version in `Cargo.lock` (`cargo binstall dioxus-cli@0.7.10`), then
`cd spikes/spike-dioxus && dx serve --hot-patch`.

## Measurements (2026-10-23)

The competition of the engine's `large_tournament` benchmark (`Tournament::large`): 1000 entrants,
random rounds over 10 rounds (5000 matches), every metric, ranked by 4 criteria.

The engine alone, `spike-core/tests/large.rs` (see its docs for how to run it), in ms:

| results reported | report one | undo / redo / load (replay) | native undo |
|---|---|---|---|
| 0 | 2.0 | 7–13 | 5 |
| 2500 | 0.4 | 760–810 | 505 |
| 4999 | 0.5 | 1680–1810 | 1120 |

Building the match rows (5000) and standings (1000) for the UI takes 2–4 ms; saving the log as JSON
takes 3–8 ms (1.4 MB at the end).

In headless Chrome, `perf/run.sh`, with all 5000 matches and 1000 standings rows on the page, 2500
results reported, in ms until the page shows the change:

| | Leptos 1x | Dioxus 1x | Leptos 4x CPU | Dioxus 4x CPU |
|---|---|---|---|---|
| load and render | 1750 | 1850 | 6660 | 7470 |
| report one result | 160–200 | 75–90 | 480–510 | 320–350 |
| undo it | 850–890 | 770–820 | 3410–3760 | 3080–3140 |
| filter to 2500 rows | 135–150 | 130–145 | 550 | 510–530 |
| back to 5000 rows | 630–720 | 370–390 | 2710–2780 | 1550–1560 |

The Leptos lists are keyed by the whole row. Keying them by position with a memo per line that
updates single cells measured slower (report 260–290 ms, filter 470–500 ms at 1x).

## Where to compare

- `spike-leptos/src/main.rs` and `spike-dioxus/src/main.rs` side by side: `view!` vs. `rsx!`,
  `Memo`s and `For` vs. components that re-run and get diffed.
- Change something in `spike-core` (e.g. add a column) and follow it into both.
