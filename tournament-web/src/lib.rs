//! The command line of [`tournament_cli`], running in the browser.
//!
//! A [`Shell`] runs the lines typed into the page against a [`Session`], which saves to the
//! browser's local storage, see [`LocalStorage`], and asks the questions configuring a phase line
//! by line. The binary wires the page up to it.

mod questions;

use std::{
    io,
    pin::Pin,
    str,
    task::{Context, Poll, Waker},
};

use plugin_factory::draft::Draft;
use tournament_cli::{
    command::{Command, ParseError},
    questionnaire,
    session::{self, Outcome, Session},
    storage::Storage,
};
use tournament_engine::PhasePlugins;
use wasm_bindgen::JsValue;

use crate::questions::{CANCEL, LinePrompter};

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
    /// The phase being configured, while its questions are asked.
    configuring: Option<Configuring>,
}

/// A phase being configured by asking its questions, see [`Shell::configure`].
struct Configuring {
    /// Fills the draft of the phase, waiting for the lines answering its questions.
    filling: Pin<Box<dyn Future<Output = Filled>>>,
    /// Shares the questions and their answers with `filling`.
    prompter: LinePrompter,
    phase: Phase,
}

/// The draft of a phase whose questions were asked, and whether all of them were answered.
type Filled = (Draft<PhasePlugins>, Result<(), questionnaire::Error>);

/// What the shell does with the phase it configures.
#[derive(Clone, Copy)]
enum Phase {
    Added,
    Edited,
}

impl Phase {
    fn apply(
        self,
        session: &mut Session,
        draft: &Draft<PhasePlugins>,
    ) -> Result<String, session::Error> {
        match self {
            Self::Added => session.add_phase(draft),
            Self::Edited => session.edit_phase(draft),
        }
    }

    fn cancelled(self) -> &'static str {
        match self {
            Self::Added => "cancelled, no phase added",
            Self::Edited => "cancelled, the phase is unchanged",
        }
    }
}

impl Shell {
    /// A shell for a new competition, which has no name yet.
    pub fn new() -> Self {
        Self {
            session: Session::new(LocalStorage),
            configuring: None,
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
        Ok(Self {
            session,
            configuring: None,
        })
    }

    /// Runs `line`, returning what it shows, in order.
    ///
    /// While a phase is configured, `line` answers the question asked, see [`Self::is_asking`].
    /// `add-phase` without a format and `edit-phase` start asking. `exit` starts a new competition,
    /// since a page cannot be left from within.
    pub fn run(&mut self, line: &str) -> Vec<Reply> {
        if let Some(configuring) = &self.configuring {
            configuring.prompter.answer(line);
            return self.resume();
        }
        let command = match Command::parse_line(line) {
            Ok(Some(command)) => command,
            Ok(None) => return Vec::new(),
            Err(ParseError::Clap(error)) => {
                let text = error.render().to_string().trim_end().to_owned();
                // clap reports help as an error, too
                return vec![if error.use_stderr() {
                    Reply::Error(text)
                } else {
                    Reply::Output(text)
                }];
            }
            Err(error) => return vec![failed(error)],
        };
        let draft = match command {
            Command::AddPhase { format: None, .. } => {
                Ok((self.session.competition().phase_draft(), Phase::Added))
            }
            Command::EditPhase => self
                .session
                .phase_draft_to_edit()
                .map(|draft| (draft, Phase::Edited)),
            command => return self.execute(command),
        };
        match draft {
            Ok((draft, phase)) => {
                let mut replies = vec![Reply::Output(format!(
                    "configuring the phase: an empty answer takes the one in brackets, \
                     `{CANCEL}` stops"
                ))];
                replies.extend(self.configure(draft, phase));
                replies
            }
            Err(error) => vec![failed(error)],
        }
    }

    fn execute(&mut self, command: Command) -> Vec<Reply> {
        match self.session.execute(command) {
            Ok(Outcome::Continue(output)) if output.is_empty() => Vec::new(),
            Ok(Outcome::Continue(output)) => vec![Reply::Output(output)],
            Ok(Outcome::Exit) => {
                *self = Self::new();
                vec![Reply::Output("started a new competition".to_owned())]
            }
            Err(error) => vec![failed(error)],
        }
    }

    /// Starts asking the questions of `draft`, the draft of the phase that is to be `phase`,
    /// starting out at its answers, and returns what the first question shows.
    fn configure(&mut self, draft: Draft<PhasePlugins>, phase: Phase) -> Vec<Reply> {
        let prompter = LinePrompter::default();
        let mut asking = prompter.clone();
        let filling = Box::pin(async move {
            let mut draft = draft;
            let result = questionnaire::fill_async(&mut draft, &mut asking).await;
            (draft, result)
        });
        self.configuring = Some(Configuring {
            filling,
            prompter,
            phase,
        });
        self.resume()
    }

    /// Asks the questions of the phase being configured up to the next one waiting for its answer,
    /// or else applies the phase. A phase whose plugins do not fit together is asked again,
    /// starting out at the answers given.
    fn resume(&mut self) -> Vec<Reply> {
        let configuring = self
            .configuring
            .as_mut()
            .expect("only a phase being configured is resumed");
        // the questions only wait for the next line, which `run` hands over before it resumes them,
        // so nothing needs to be woken
        let mut context = Context::from_waker(Waker::noop());
        let Poll::Ready((draft, filled)) = configuring.filling.as_mut().poll(&mut context) else {
            return configuring.prompter.shown();
        };
        let mut replies = configuring.prompter.shown();
        let phase = configuring.phase;
        self.configuring = None;
        match filled {
            Ok(()) => match phase.apply(&mut self.session, &draft) {
                Ok(output) => replies.push(Reply::Output(output)),
                Err(error @ session::Error::InvalidPhase(_)) => {
                    replies.push(Reply::Error(format!("error: {error}\nplease answer again")));
                    replies.extend(self.configure(draft, phase));
                }
                Err(error) => replies.push(failed(error)),
            },
            Err(questionnaire::Error::Cancelled) => {
                replies.push(Reply::Output(phase.cancelled().to_owned()));
            }
            Err(error) => replies.push(Reply::Error(format!(
                "error: cannot configure the phase: {error}"
            ))),
        }
        replies
    }

    /// Whether the questions configuring a phase are asked, which the lines run answer.
    pub fn is_asking(&self) -> bool {
        self.configuring.is_some()
    }

    /// Names the competition and marks unsaved changes with a `*`, e.g. `cup*> `. While a question
    /// is asked, it says what the question takes instead, e.g. `number [3]> `.
    pub fn prompt(&self) -> String {
        if let Some(configuring) = &self.configuring {
            return configuring.prompter.prompt();
        }
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

/// `error` as a line that was not executed shows it.
fn failed(error: impl std::error::Error) -> Reply {
    Reply::Error(format!("error: {error}"))
}

// the local storage only exists in a browser
#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

    use super::*;

    wasm_bindgen_test_configure!(run_in_browser);

    /// What `line` shows, which has to be some output and no error, a line for each reply.
    #[track_caller]
    fn output(shell: &mut Shell, line: &str) -> String {
        let replies = shell.run(line);
        let outputs: Vec<_> = replies
            .iter()
            .map(|reply| match reply {
                Reply::Output(output) => output.as_str(),
                Reply::Error(_) => panic!("{line:?} must not fail, got {replies:?}"),
            })
            .collect();
        assert!(!outputs.is_empty(), "{line:?} must show an output");
        outputs.join("\n")
    }

    /// Takes the answer every question offers until the questions end, returning what the last
    /// answer shows.
    #[track_caller]
    fn accept_every_question(shell: &mut Shell) -> String {
        for _ in 0..100 {
            let shown = output(shell, "");
            if !shell.is_asking() {
                return shown;
            }
        }
        panic!("the questions must end");
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
        assert_eq!(Shell::new().run("  "), []);
    }

    #[wasm_bindgen_test]
    fn help_is_shown_rather_than_reported_as_an_error() {
        let help = output(&mut Shell::new(), "help");

        assert!(help.contains("add-entrant"), "{help}");
    }

    #[wasm_bindgen_test]
    fn an_unknown_command_is_an_error() {
        assert!(matches!(Shell::new().run("dance")[..], [Reply::Error(_)]));
    }

    #[wasm_bindgen_test]
    fn a_failing_command_is_an_error() {
        assert_eq!(
            Shell::new().run("remove Alice"),
            [Reply::Error(
                "error: there is no entrant named \"Alice\"".to_owned()
            )]
        );
    }

    #[wasm_bindgen_test]
    fn exit_starts_a_new_competition() {
        let mut shell = Shell::new();
        output(&mut shell, "add Alice");

        assert!(matches!(shell.run("exit")[..], [Reply::Error(_)]));
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
    fn a_phase_without_a_format_is_configured_by_its_questions() {
        let mut shell = Shell::new();

        let asked = output(&mut shell, "add-phase");
        assert!(asked.contains("Format\n"), "{asked}");
        assert!(asked.contains("1. round robin"), "{asked}");
        assert_eq!(shell.prompt(), "number [1]> ");

        assert_eq!(
            accept_every_question(&mut shell),
            "phase added: round robin, ranked by more points, then more Sonneborn-Berger"
        );
        assert_eq!(shell.prompt(), "tournament*> ");
    }

    #[wasm_bindgen_test]
    fn an_option_is_chosen_by_its_name() {
        let mut shell = Shell::new();
        output(&mut shell, "add-phase");

        let asked = output(&mut shell, "Random Rounds");

        assert!(asked.starts_with("Random rounds: rounds"), "{asked}");
    }

    #[wasm_bindgen_test]
    fn an_answer_that_does_not_answer_the_question_is_asked_again() {
        let mut shell = Shell::new();
        output(&mut shell, "add-phase");

        let replies = shell.run("seven");

        let [Reply::Error(complaint)] = &replies[..] else {
            panic!("the answer must only be complained about, got {replies:?}");
        };
        assert!(
            complaint.starts_with("answer with the number"),
            "{complaint}"
        );
        assert!(shell.is_asking());
        assert_eq!(shell.prompt(), "number [1]> ");
    }

    #[wasm_bindgen_test]
    fn a_number_shows_its_range_and_one_out_of_it_is_asked_again() {
        let mut shell = Shell::new();
        output(&mut shell, "add-phase");

        let asked = output(&mut shell, "random rounds");
        assert!(
            asked.ends_with("takes a whole number of at least 1"),
            "{asked}"
        );
        let replies = shell.run("0");

        let [Reply::Error(complaint)] = &replies[..] else {
            panic!("the answer must only be complained about, got {replies:?}");
        };
        assert_eq!(complaint, "this takes a whole number of at least 1");
        assert_eq!(shell.prompt(), "whole number [5]> ");
    }

    #[wasm_bindgen_test]
    fn cancel_stops_the_questions_without_adding_a_phase() {
        let mut shell = Shell::new();
        output(&mut shell, "add-phase");
        output(&mut shell, "1");

        assert_eq!(output(&mut shell, "cancel"), "cancelled, no phase added");
        assert!(!shell.is_asking());
        output(&mut shell, "add-phase round-robin");
    }

    #[wasm_bindgen_test]
    fn editing_the_phase_starts_out_at_its_configuration() {
        let mut shell = Shell::new();
        output(&mut shell, "add-phase random-rounds rounds=3");
        output(&mut shell, "edit-phase");

        // the format, then its rounds
        output(&mut shell, "");
        assert_eq!(shell.prompt(), "whole number [3]> ");
        output(&mut shell, "5");

        assert!(
            accept_every_question(&mut shell)
                .starts_with("phase changed: random rounds (rounds: 5)")
        );
    }

    #[wasm_bindgen_test]
    fn editing_without_a_phase_asks_nothing() {
        let mut shell = Shell::new();

        assert!(matches!(shell.run("edit-phase")[..], [Reply::Error(_)]));
        assert!(!shell.is_asking());
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
