# tournament-web

The command line of [`tournament-cli`](../tournament-cli), running in the
browser: the engine is compiled to WebAssembly, so everything runs locally in
the page, and nothing is sent anywhere.

```text
cd tournament-web
trunk serve --open    # or: trunk build, then serve dist/ with any static web server
```

This needs [trunk](https://trunkrs.dev) and the `wasm32-unknown-unknown` target
(`rustup target add wasm32-unknown-unknown`). The built page is a static
directory and cannot be opened from the file system directly, since browsers do
not load WebAssembly from `file://` URLs.

## Differences from the terminal

The commands are those of the CLI (type `help`), except:

- A competition is saved in the local storage of the browser, under the name
  given to `save`. The part of the URL after `#` names it, like the file
  argument of the CLI: `index.html#cup` opens the competition saved as `cup`,
  and saving sets the URL to the name saved under, so reloading the page opens
  it again.
- `add-phase` without a format and `edit-phase` ask their questions line by
  line: each question lists its options by number, and the prompt says what it
  takes, with the answer it offers in brackets, e.g. `number [3]>`. An option is
  chosen by its number or its name, several by their numbers separated by
  spaces (or `none`). An empty answer takes the one in brackets, and `cancel`
  stops the questions.
- `exit` starts a new competition, still refusing to discard unsaved changes
  unless forced. Leaving the page with unsaved changes asks first.
- The arrow keys walk through the lines typed so far.

## End-to-end tests

[`tournament-web-e2e`](../tournament-web-e2e) types into the page in headless
Chrome. It needs Chrome installed, and fetches the matching chromedriver
itself. The tests are ignored by a plain `cargo test`, and run on the page as
last built:

```text
(cd tournament-web && trunk build) && cargo test -p tournament-web-e2e -- --ignored
```

With `E2E_HEADED=1`, Chrome opens a window for each test, which shows what it
does. `E2E_PAUSE=<seconds>` then slows the test down, pausing that long after
each of its actions (a line typed, then run, a key pressed, the page opened,
reloaded or left), and `E2E_LINGER=<seconds>` keeps the window open that long
after the test, passed or failed. The tests of a file run in parallel; to watch
them one after the other, add `--test-threads=1`, or name a single test after
`-p tournament-web-e2e`:

```text
E2E_HEADED=1 E2E_PAUSE=0.5 E2E_LINGER=5 cargo test -p tournament-web-e2e the_arrow_keys -- --ignored
```
