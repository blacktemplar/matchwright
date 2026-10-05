//! The commands an operator types at the prompt, and how a line of input becomes one.

use std::{num::NonZeroUsize, path::PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use thiserror::Error;

/// A single command typed at the prompt.
///
/// The doc comment of every variant is the help text the prompt shows for it, while this one is
/// kept out of it.
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
    /// Add the phase the competition is played in
    AddPhase {
        #[command(subcommand)]
        preset: PhasePreset,
    },
    /// Start the competition, which schedules the first matches
    Start,
    /// List the matches scheduled so far, with their number and result
    Matches {
        /// Only list the matches that have no result yet
        #[arg(long)]
        open: bool,
    },
    /// Report the result of a match
    Report {
        /// The number of the match, as listed by `matches`
        number: NonZeroUsize,
        result: ReportedResult,
    },
    /// Show the current ranking
    Ranking,
    /// Undo the last change
    Undo,
    /// Redo the last undone change
    Redo,
    /// Save the competition, to the file it was loaded from or last saved to unless a path is given
    Save { path: Option<PathBuf> },
    /// Exit; refuses if there are unsaved changes, unless forced
    #[command(visible_alias = "quit")]
    Exit {
        /// Exit even if there are unsaved changes, discarding them
        #[arg(long)]
        force: bool,
    },
}

/// The phase formats the CLI offers, each with a fixed set of metrics it is ranked by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum PhasePreset {
    /// Everyone plays everyone; ranked by points (3 for a win, 1 for a draw), then
    /// Sonneborn-Berger
    RoundRobin,
    /// The loser of a match is out, draws are not allowed; ranked by the round of the first loss
    SingleElimination,
    /// A number of randomly chosen rounds of a round robin; ranked by points (3 for a win, 1 for a
    /// draw), then Buchholz, Sonneborn-Berger and Buchholz of Buchholz
    RandomRounds { rounds: u32 },
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
    /// Covers both a malformed command and an explicit request for help, which clap reports as an
    /// error too; [`clap::Error::print`] tells them apart when showing it.
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
    fn a_phase_preset_takes_its_own_arguments() {
        assert_eq!(
            parse("add-phase random-rounds 5"),
            Command::AddPhase {
                preset: PhasePreset::RandomRounds { rounds: 5 }
            }
        );
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
    fn match_numbers_start_at_one() {
        assert!(Command::parse_line("report 0 a").is_err());
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
