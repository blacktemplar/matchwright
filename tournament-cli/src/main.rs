//! An interactive command line interface for running a tournament with [`tournament_engine`].
//!
//! ```text
//! tournament-cli [FILE]
//! ```
//!
//! Loads the competition saved to `FILE` if it exists, and otherwise starts a new one that is
//! saved there. Type `help` at the prompt for the available commands.

use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    process::ExitCode,
};

use clap::Parser;
use inquire::InquireError;
use rustyline::{DefaultEditor, error::ReadlineError};
use thiserror::Error;
use tournament_cli::{
    command::{Command, ParseError},
    questionnaire::{self, Terminal},
    session::{self, Outcome, Session},
};

/// Runs a tournament interactively.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// The file the competition is loaded from, if it exists, and saved to
    file: Option<PathBuf>,
}

/// Why the session ended other than by an `exit`.
#[derive(Debug, Error)]
enum Error {
    #[error(transparent)]
    Session(#[from] session::Error),
    #[error("cannot configure the phase: {0}")]
    Configure(questionnaire::Error),
    #[error("cannot read from the terminal: {0}")]
    Terminal(#[from] ReadlineError),
    #[error("cannot write to the terminal: {0}")]
    Output(#[from] io::Error),
    #[error("the input ended with unsaved changes, which are discarded")]
    InputEnded,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let session = match args.file {
        Some(path) if path.exists() => Session::load(path),
        path => Ok(Session::new(path)),
    };
    match session.map_err(Error::from).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Reads and executes commands until one ends the session.
fn run(mut session: Session) -> Result<(), Error> {
    let mut editor = DefaultEditor::new()?;
    loop {
        let command = match editor.readline(&prompt(&session)) {
            Ok(line) => {
                editor.add_history_entry(line.as_str())?;
                match Command::parse_line(&line) {
                    Ok(Some(command)) => command,
                    Ok(None) => continue,
                    Err(ParseError::Clap(error)) => {
                        // also shows help, which clap reports as an error
                        error.print()?;
                        continue;
                    }
                    Err(error) => {
                        eprintln!("error: {error}");
                        continue;
                    }
                }
            }
            // Ctrl-C only discards the line being typed
            Err(ReadlineError::Interrupted) => continue,
            // Ctrl-D exits, but not past unsaved changes
            Err(ReadlineError::Eof) if io::stdin().is_terminal() => Command::Exit { force: false },
            // input that is not a terminal ends for good, and every further read would end the
            // same way, so unsaved changes can neither be saved nor kept
            Err(ReadlineError::Eof) if session.has_unsaved_changes() => {
                return Err(Error::InputEnded);
            }
            Err(ReadlineError::Eof) => return Ok(()),
            Err(error) => return Err(Error::Terminal(error)),
        };
        // a session never asks the operator anything, so a phase that is to be configured is
        // configured here and reaches the session as the configuration it produced
        let outcome = match command {
            Command::AddPhase { preset: None } => add_configured_phase(&mut session),
            command => session.execute(command).map_err(Error::Session),
        };
        match outcome {
            Ok(Outcome::Continue(output)) if output.is_empty() => {}
            Ok(Outcome::Continue(output)) => println!("{output}"),
            Ok(Outcome::Exit) => return Ok(()),
            Err(error) => eprintln!("error: {error}"),
        }
    }
}

/// Configures a phase at the prompt and adds it, leaving the competition untouched if the
/// operator cancels the configuration. A phase whose plugins do not fit together is configured
/// again, starting out at the answers given.
fn add_configured_phase(session: &mut Session) -> Result<Outcome, Error> {
    let mut draft = session.competition().phase_draft();
    loop {
        match questionnaire::fill(&mut draft, &mut Terminal) {
            Ok(()) => {}
            Err(questionnaire::Error::Prompt(
                InquireError::OperationCanceled | InquireError::OperationInterrupted,
            )) => return Ok(Outcome::Continue("cancelled, no phase added".to_owned())),
            // input that is piped in cannot answer any question, which is what the session says
            // when it is handed an unconfigured phase itself
            Err(questionnaire::Error::Prompt(InquireError::NotTTY)) => {
                return Err(Error::Session(session::Error::NotConfigured));
            }
            Err(error) => return Err(Error::Configure(error)),
        }
        match session.add_phase(&draft) {
            Err(error @ session::Error::Unsettled(_)) => {
                eprintln!("error: {error}\nplease answer again");
            }
            added => return added.map(Outcome::Continue).map_err(Error::Session),
        }
    }
}

/// Names the file the competition is saved to and marks unsaved changes with a `*`.
fn prompt(session: &Session) -> String {
    let name = session
        .path()
        .and_then(|path| path.file_stem())
        .map_or("tournament".into(), |stem| stem.to_string_lossy());
    let unsaved = if session.has_unsaved_changes() {
        "*"
    } else {
        ""
    };
    format!("{name}{unsaved}> ")
}
