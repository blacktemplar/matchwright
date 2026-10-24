# tournament-gui

The GUI for running a tournament, in the browser, on
[`tournament-app`](../tournament-app). For now one screen: enter the entrants,
add a phase in a format with its defaults, start, report and reopen results
round by round, undo and redo, and follow the standings. The competition is
kept in memory only, and is lost with the page.

```text
dx serve -p tournament-gui              # add --hot-patch to patch Rust code into the running page
dx build -p tournament-gui --release    # into target/dx/tournament-gui/release/web/public
```

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
