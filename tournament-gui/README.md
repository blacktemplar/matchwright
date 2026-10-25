# tournament-gui

The GUI for running a tournament, in the browser, on
[`tournament-app`](../tournament-app). It lists the competitions stored in the
browser (IndexedDB, through [`tournament-browser`](../tournament-browser)) to
open or export, and starts a new one or imports one from a file (also one the
CLI saved). A competition open, named after the `#` of the URL: enter the
entrants, configure the phase in a form (its format and the fields of the
format, the criteria it is ranked by and the metrics calculated, starting with
what the format recommends), start, report and reopen results round by round
or among the matches playable now (by keyboard too: type a match's number and
`1`, `X` or `2`, e.g. `3 1`, or move from one result button to the next), edit
the phase in a dialog, undo and redo (each announced), and follow the
standings. Every change
is saved at once. One tab at a time changes a competition; another tab shows it
read-only, follows the changes, and takes over when that tab leaves it.

It speaks English and German: the language chosen last in the browser, or else
the first the browser prefers. Its messages are in `locales/<lang>/gui.ftl`
([Fluent](https://projectfluent.org)); those of errors and plugins are in
[`tournament-app`](../tournament-app/locales).

```text
dx serve -p tournament-gui              # add --hot-patch to patch Rust code into the running page
dx build -p tournament-gui --release    # into target/dx/tournament-gui/release/web/public
```

The select and the dialog are the Dioxus team's components, copied into
`src/components/` by `dx components add` and changed there.

This needs the Dioxus CLI `dx` of the version of `dioxus` in `Cargo.lock`
(`cargo binstall dioxus-cli@0.7.10`) and the `wasm32-unknown-unknown` target.

## End-to-end tests

[`tournament-gui-e2e`](../tournament-gui-e2e) uses the page in headless Chrome,
like the tests of [`tournament-web`](../tournament-web/README.md#end-to-end-tests)
(the same environment variables), and checks its accessibility with axe-core at
the end of every test. They run on the release build, since a debug build tries
to reach the server of `dx serve`:

```text
dx build -p tournament-gui --release && cargo test -p tournament-gui-e2e -- --ignored
```
