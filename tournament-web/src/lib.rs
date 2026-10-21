//! The command line of [`tournament_cli`], running in the browser.
//!
//! A [`Shell`] runs the lines typed into the page against a [`Session`], which saves to the
//! browser's local storage, see [`LocalStorage`]. The binary wires the page up to it.

use std::{io, str};

use tournament_cli::{
    command::{Command, ParseError},
    session::{self, Outcome, Session},
    storage::Storage,
};
use wasm_bindgen::JsValue;

/// What the key of a competition in the local storage starts with, followed by its name.
const KEY_PREFIX: &str = "tournament-web:";

/// Saves in the local storage of the browser, so that a competition outlives the page.
pub struct LocalStorage;

impl LocalStorage {
    fn storage() -> io::Result<web_sys::Storage> {
        web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "the browser offers no local storage",
                )
            })
    }
}

impl Storage for LocalStorage {
    fn read(&self, name: &str) -> io::Result<Vec<u8>> {
        let item = Self::storage()?.get_item(&key(name)).map_err(js_error)?;
        item.map(String::into_bytes)
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    fn write(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let text = str::from_utf8(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Self::storage()?
            .set_item(&key(name), text)
            .map_err(js_error)
    }
}

fn key(name: &str) -> String {
    format!("{KEY_PREFIX}{name}")
}

fn js_error(error: JsValue) -> io::Error {
    io::Error::other(format!("{error:?}"))
}

/// What a line run by a [`Shell`] shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// What the command shows.
    Output(String),
    /// Why the line was not executed.
    Error(String),
}

/// Runs the lines typed into the page against a session saving to the [`LocalStorage`].
pub struct Shell {
    session: Session,
}

impl Shell {
    /// A shell for a new competition, which has no name yet.
    pub fn new() -> Self {
        Self {
            session: Session::new(LocalStorage),
        }
    }

    /// A shell for the competition saved under `name`, or else for a new one that is saved under
    /// it.
    ///
    /// # Errors
    /// Returns an error if something is saved under `name` but cannot be loaded, e.g. because it is
    /// no competition.
    pub fn open(name: &str) -> Result<Self, session::Error> {
        let session = match Session::load(LocalStorage, name) {
            Err(session::Error::Read { source, .. })
                if source.kind() == io::ErrorKind::NotFound =>
            {
                Session::new(LocalStorage).named(name)
            }
            session => session?,
        };
        Ok(Self { session })
    }

    /// Runs `line`, returning what it shows, if anything.
    ///
    /// `exit` starts a new competition, since a page cannot be left from within.
    pub fn run(&mut self, line: &str) -> Option<Reply> {
        let command = match Command::parse_line(line) {
            Ok(Some(command)) => command,
            Ok(None) => return None,
            Err(ParseError::Clap(error)) => {
                let text = error.render().to_string().trim_end().to_owned();
                // clap reports help as an error, too
                return Some(if error.use_stderr() {
                    Reply::Error(text)
                } else {
                    Reply::Output(text)
                });
            }
            Err(error) => return Some(Reply::Error(format!("error: {error}"))),
        };
        match self.session.execute(command) {
            Ok(Outcome::Continue(output)) if output.is_empty() => None,
            Ok(Outcome::Continue(output)) => Some(Reply::Output(output)),
            Ok(Outcome::Exit) => {
                *self = Self::new();
                Some(Reply::Output("started a new competition".to_owned()))
            }
            Err(error) => Some(Reply::Error(format!("error: {error}"))),
        }
    }

    /// Names the competition and marks unsaved changes with a `*`, e.g. `cup*> `.
    pub fn prompt(&self) -> String {
        let name = self.session.name().unwrap_or("tournament");
        let unsaved = if self.session.has_unsaved_changes() {
            "*"
        } else {
            ""
        };
        format!("{name}{unsaved}> ")
    }

    /// The name the competition is saved under unless a save is given another one.
    pub fn name(&self) -> Option<&str> {
        self.session.name()
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.session.has_unsaved_changes()
    }
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

// the local storage only exists in a browser
#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

    use super::*;

    wasm_bindgen_test_configure!(run_in_browser);

    /// The output of `line`, which has to show some.
    #[track_caller]
    fn output(shell: &mut Shell, line: &str) -> String {
        match shell.run(line) {
            Some(Reply::Output(output)) => output,
            other => panic!("{line:?} must show an output, got {other:?}"),
        }
    }

    /// Removes what the test saved under `name` once it is dropped, so that tests leave the local
    /// storage as they found it.
    struct Saved(&'static str);

    impl Drop for Saved {
        fn drop(&mut self) {
            LocalStorage::storage()
                .unwrap()
                .remove_item(&key(self.0))
                .unwrap();
        }
    }

    #[wasm_bindgen_test]
    fn a_line_shows_what_its_command_shows() {
        let mut shell = Shell::new();

        assert_eq!(
            output(&mut shell, "add Alice"),
            "1 entrant(s) added, 1 in total"
        );
    }

    #[wasm_bindgen_test]
    fn an_empty_line_shows_nothing() {
        assert_eq!(Shell::new().run("  "), None);
    }

    #[wasm_bindgen_test]
    fn help_is_shown_rather_than_reported_as_an_error() {
        let help = output(&mut Shell::new(), "help");

        assert!(help.contains("add-entrant"), "{help}");
    }

    #[wasm_bindgen_test]
    fn an_unknown_command_is_an_error() {
        assert!(matches!(Shell::new().run("dance"), Some(Reply::Error(_))));
    }

    #[wasm_bindgen_test]
    fn a_failing_command_is_an_error() {
        assert_eq!(
            Shell::new().run("remove Alice"),
            Some(Reply::Error(
                "error: there is no entrant named \"Alice\"".to_owned()
            ))
        );
    }

    #[wasm_bindgen_test]
    fn exit_starts_a_new_competition() {
        let mut shell = Shell::new();
        output(&mut shell, "add Alice");

        assert!(matches!(shell.run("exit"), Some(Reply::Error(_))));
        output(&mut shell, "exit --force");

        assert_eq!(output(&mut shell, "entrants"), "there are no entrants yet");
    }

    #[wasm_bindgen_test]
    fn the_prompt_marks_unsaved_changes() {
        let mut shell = Shell::new();
        assert_eq!(shell.prompt(), "tournament> ");

        output(&mut shell, "add Alice");

        assert_eq!(shell.prompt(), "tournament*> ");
    }

    #[wasm_bindgen_test]
    fn a_saved_competition_opens_again() {
        let saved = Saved("opens-again");
        let mut shell = Shell::new();
        output(&mut shell, "add Alice");
        output(&mut shell, &format!("save {}", saved.0));

        let opened = Shell::open(saved.0).unwrap();

        assert_eq!(opened.prompt(), "opens-again> ");
        assert_eq!(output(&mut { opened }, "entrants"), "Alice");
    }

    #[wasm_bindgen_test]
    fn a_name_nothing_is_saved_under_opens_a_new_competition_saved_under_it() {
        let saved = Saved("new-cup");
        let mut shell = Shell::open(saved.0).unwrap();
        output(&mut shell, "add Alice");

        output(&mut shell, "save");

        assert_eq!(
            output(&mut Shell::open(saved.0).unwrap(), "entrants"),
            "Alice"
        );
    }

    #[wasm_bindgen_test]
    fn what_is_no_competition_is_not_opened() {
        let saved = Saved("notes");
        LocalStorage.write(saved.0, b"{}").unwrap();

        assert!(matches!(
            Shell::open(saved.0),
            Err(session::Error::Load { .. })
        ));
    }
}
