//! The commands an operator types at the prompt, and how a line of input becomes one.

use std::num::{NonZeroU32, NonZeroUsize};

use clap::{Parser, ValueEnum};
use thiserror::Error;

/// A single command typed at the prompt.
///
/// The doc comment of every variant is the help text the prompt shows for it. This one is kept out
/// of the help.
#[derive(Debug, Clone, PartialEq, Parser)]
#[command(multicall = true, about = None, long_about = None)]
pub enum Command {
    /// List the entrants of the competition
    Entrants,
    /// Add one or more entrants, each given by the name it is shown and addressed by
    #[command(visible_alias = "add")]
    AddEntrant {
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// Remove an entrant from the competition
    #[command(visible_alias = "remove")]
    RemoveEntrant { name: String },
    /// Add the phase the competition is played in, configuring it at the prompt unless a format
    /// is given
    AddPhase {
        /// The format, by its label with hyphens for spaces (e.g. `round-robin`) or by its id;
        /// everything else is what the format recommends
        format: Option<String>,
        /// Answers to the fields of the format, e.g. `rounds=7`; a field not answered here is
        /// answered as it offers
        #[arg(requires = "format", value_name = "FIELD=VALUE")]
        answers: Vec<String>,
    },
    /// Change the configuration of the phase at the prompt, starting out at its current one; the
    /// results reported so far stay as long as the changed phase schedules their matches
    EditPhase,
    /// Start the competition, which schedules the first matches
    Start,
    /// List the matches scheduled so far, with their number and result
    Matches {
        /// Only list the matches that have no result yet
        #[arg(long)]
        open: bool,
        /// Only list the matches of this round
        #[arg(long)]
        round: Option<NonZeroU32>,
    },
    /// Report the result of a match
    Report {
        /// The number of the match, as listed by `matches`
        number: NonZeroUsize,
        /// Case insensitive, so that the sides can be named the way `matches` shows them
        #[arg(ignore_case = true)]
        result: ReportedResult,
    },
    /// Undo the result reported for a match, which may be any reported match rather than only the
    /// one reported last
    UndoReport {
        /// The number of the match, as listed by `matches`
        number: NonZeroUsize,
    },
    /// Show the current ranking
    Ranking,
    /// Undo the last change
    Undo,
    /// Redo the last undone change
    Redo,
    /// Save the competition, under the name it was loaded or last saved under unless one is given
    Save { name: Option<String> },
    /// Exit; refuses if there are unsaved changes, unless forced
    #[command(visible_alias = "quit")]
    Exit {
        /// Exit even if there are unsaved changes, discarding them
        #[arg(long)]
        force: bool,
    },
}

/// The result of a match, as reported by an operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReportedResult {
    /// The first team won
    A,
    /// The second team won
    B,
    /// The match ended in a draw
    Draw,
}

/// Why a line of input is not a command.
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("the line has an unclosed quote or a trailing backslash")]
    Unquote,
    /// Covers both a malformed command and an explicit request for help, which clap also reports
    /// as an error. [`clap::Error::print`] shows either of them correctly.
    #[error(transparent)]
    Clap(#[from] clap::Error),
}

impl Command {
    /// Parses a line typed at the prompt, splitting it into words the way a POSIX shell would, so
    /// that a name containing a space can be quoted.
    ///
    /// Returns `None` for a line without any words, which is no command but not an error either.
    ///
    /// # Errors
    /// Returns an error if the line cannot be split into words or the words are not a valid
    /// command, which includes a request for help.
    ///
    /// # Examples
    /// ```
    /// use tournament_cli::command::Command;
    ///
    /// assert_eq!(
    ///     Command::parse_line(r#"add-entrant "Ada Lovelace""#)?,
    ///     Some(Command::AddEntrant {
    ///         names: vec!["Ada Lovelace".to_owned()]
    ///     })
    /// );
    /// assert_eq!(Command::parse_line("   ")?, None);
    /// # Ok::<(), tournament_cli::command::ParseError>(())
    /// ```
    pub fn parse_line(line: &str) -> Result<Option<Self>, ParseError> {
        let words = shlex::split(line).ok_or(ParseError::Unquote)?;
        if words.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self::try_parse_from(words)?))
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, error::ErrorKind};
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use super::*;

    #[track_caller]
    fn parse(line: &str) -> Command {
        Command::parse_line(line)
            .expect("line must parse")
            .expect("line must hold a command")
    }

    #[test]
    fn the_command_definition_is_consistent() {
        Command::command().debug_assert();
    }

    #[test]
    fn several_entrants_can_be_added_at_once() {
        assert_eq!(
            parse("add Alice 'Bob B.'"),
            Command::AddEntrant {
                names: vec!["Alice".to_owned(), "Bob B.".to_owned()]
            }
        );
    }

    #[test]
    fn adding_no_entrant_is_rejected() {
        assert!(Command::parse_line("add-entrant").is_err());
    }

    #[test]
    fn a_format_takes_answers_to_its_fields() {
        assert_eq!(
            parse("add-phase random-rounds rounds=5"),
            Command::AddPhase {
                format: Some("random-rounds".to_owned()),
                answers: vec!["rounds=5".to_owned()],
            }
        );
    }

    /// A phase is configured at the prompt rather than given by its format, which is what the
    /// absent format stands for.
    #[test]
    fn add_phase_takes_no_format_at_all() {
        assert_eq!(
            parse("add-phase"),
            Command::AddPhase {
                format: None,
                answers: Vec::new(),
            }
        );
    }

    #[test]
    fn matches_can_be_narrowed_down_by_both_filters() {
        assert_eq!(
            parse("matches --open --round 2"),
            Command::Matches {
                open: true,
                round: NonZeroU32::new(2)
            }
        );
        assert_eq!(
            parse("matches"),
            Command::Matches {
                open: false,
                round: None
            }
        );
    }

    #[test]
    fn rounds_start_at_one() {
        assert!(Command::parse_line("matches --round 0").is_err());
    }

    #[test]
    fn a_result_is_reported_by_match_number() {
        assert_eq!(
            parse("report 3 draw"),
            Command::Report {
                number: NonZeroUsize::new(3).unwrap(),
                result: ReportedResult::Draw
            }
        );
    }

    #[test]
    fn a_result_can_be_reported_in_the_case_the_sides_are_shown_in() {
        assert_eq!(
            parse("report 1 A"),
            Command::Report {
                number: NonZeroUsize::new(1).unwrap(),
                result: ReportedResult::A
            }
        );
        assert_eq!(
            parse("report 1 B"),
            Command::Report {
                number: NonZeroUsize::new(1).unwrap(),
                result: ReportedResult::B
            }
        );
        assert_eq!(
            parse("report 1 Draw"),
            Command::Report {
                number: NonZeroUsize::new(1).unwrap(),
                result: ReportedResult::Draw
            }
        );
    }

    #[test]
    fn match_numbers_start_at_one() {
        assert!(Command::parse_line("report 0 a").is_err());
        assert!(Command::parse_line("undo-report 0").is_err());
    }

    #[test]
    fn a_reported_result_is_undone_by_match_number() {
        assert_eq!(
            parse("undo-report 3"),
            Command::UndoReport {
                number: NonZeroUsize::new(3).unwrap()
            }
        );
    }

    #[test]
    fn help_is_reported_as_a_clap_error_to_be_printed() {
        let Err(ParseError::Clap(error)) = Command::parse_line("help") else {
            panic!("help must be reported through clap");
        };

        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn an_unclosed_quote_is_rejected() {
        assert!(matches!(
            Command::parse_line("add 'Alice"),
            Err(ParseError::Unquote)
        ));
    }
}
