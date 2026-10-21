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
