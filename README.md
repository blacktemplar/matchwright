# Matchwright

Run a tournament in your browser: enter the players, pick a format, report the
results and follow the standings or the bracket. Matchwright works offline and
keeps everything on your device: there is no account, no server and no tracking.

**Open it: <https://blacktemplar.github.io/matchwright/>**

## What it does

- **Formats:** round robin, random rounds of a round robin, single elimination
  (with an optional match for third place), double elimination (with an
  optional grand final reset) and Monster DYP (teams of two drawn anew every
  round).
- **Rankings** by criteria you choose and order: points (for a win, a draw and
  a bye, as you set them), wins, draws, losses, Buchholz, Buchholz of Buchholz,
  Sonneborn-Berger, and for knockouts how far an entrant came.
- **Reporting results** with a tap on the winner, in the bracket, or by
  keyboard (`3 1` for match 3 won by the first side, `ali` for the open match
  Alice is in); every result can be undone.
- **Standings, a bracket and a cross table**, laid out for phones, tablets and
  laptops, in light or dark.
- **Offline and installable** as an app. Competitions are stored in the
  browser; export one as a file to back it up or move it to another device.
  Several tabs can show the same competition; one of them changes it at a time.
- **English and German.**
- **Accessible:** built to WCAG 2.1 AA, usable by keyboard and screen reader.

## Privacy

Nothing you enter leaves your device: competitions and names are stored in
your browser's IndexedDB and nowhere else. There is no tracking, no analytics
and no cookies. The page is a static site: once loaded it makes no requests
except to update itself. It is served by GitHub Pages, which, like any web
server, receives your IP address when it delivers the page (see
[GitHub's privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement)).

## For developers

Matchwright is written in Rust and runs as WebAssembly. The repository is a
Cargo workspace:

| Crate | What it is |
|---|---|
| [`tournament-engine`](tournament-engine) | the library: competitions, phases, matches and rankings, with pluggable formats, ranking criteria and metrics, and every change kept as a log that replays |
| [`plugin-factory`](plugin-factory) | plugin axes, their catalog, schemas, drafts and compatibility checks, which the engine builds on |
| [`tournament-app`](tournament-app) | what the GUI does, without any UI: sessions, forms, views, translation and storage |
| [`tournament-gui`](tournament-gui) | the page itself, in [Dioxus](https://dioxuslabs.com) |
| [`tournament-browser`](tournament-browser) | what the GUI needs of the browser: IndexedDB, locks, channels between tabs, files |
| [`tournament-cli`](tournament-cli) | an interactive command line on the engine |
| [`tournament-web`](tournament-web) | that command line in the browser |

`tournament-gui-e2e`, `tournament-web-e2e` and `e2e-harness` hold the
end-to-end tests, which drive the pages in headless Chrome.

```sh
dx serve -p tournament-gui            # the page, at http://localhost:8080
cargo run -p tournament-cli           # the command line
prek run --all-files                  # formatting, lints and tests, as CI runs them
```

The page needs [`dx`](https://dioxuslabs.com/learn/0.7/getting_started) of the
version of `dioxus` in `Cargo.lock`, and the `wasm32-unknown-unknown` target.
[`DESIGN.md`](DESIGN.md) records the design decisions and their reasons,
[`AGENTS.md`](AGENTS.md) describes the commands and architecture in detail, and
[`CHANGELOG.md`](CHANGELOG.md) what changed in each version.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
