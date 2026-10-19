//! The commands an operator types at the prompt, and how a line of input becomes one.

use std::{
    num::{NonZeroU32, NonZeroUsize},
    path::PathBuf,
};

use clap::{Parser, Subcommand, ValueEnum};
use thiserror::Error;
use tournament_engine::plugins::phase_format::{
    DoubleElimination, MonsterDyp, PhaseFormat, RandomRounds, RoundRobin, SingleElimination,
};

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
    /// Add the phase the competition is played in, configuring it at the prompt unless a preset
    /// is given
    AddPhase {
        #[command(subcommand)]
        preset: Option<PhasePreset>,
    },
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

/// A phase format together with the metrics and the ranking it comes with, which is what
/// `add-phase` adds without asking anything.
///
/// A preset is the format it names, with every other part of the phase answered as the format
/// recommends, see [`AcceptDefaults`](crate::questionnaire::AcceptDefaults).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum PhasePreset {
    /// Everyone plays everyone; ranked by points (1 for a win or a bye, 0.5 for a draw), then
    /// Sonneborn-Berger
    RoundRobin,
    /// The loser of a match is out, draws are not allowed; the semifinal losers play for third
    /// place and are ranked by the round of the first loss, then the losses
    SingleElimination {
        /// End the bracket with the final, leaving the two semifinal losers third equal
        #[arg(long)]
        no_third_place_match: bool,
    },
    /// An entrant is out after its second loss, draws are not allowed; the champions of the
    /// winner and the loser bracket meet in the grand final, which is played once more if the
    /// champion of the winner bracket loses it; ranked by the round of elimination, then the losses
    DoubleElimination {
        /// Make the grand final a single match, so that losing it knocks out even the champion of
        /// the winner bracket
        #[arg(long)]
        no_grand_final_reset: bool,
    },
    /// A number of randomly chosen rounds of a round robin; ranked by points (1 for a win or a bye,
    /// 0.5 for a draw), then Buchholz, Sonneborn-Berger and Buchholz of Buchholz
    RandomRounds { rounds: NonZeroU32 },
    /// Every round, the entrants are drawn into random teams of two, each playing another such
    /// team, and the ones left over sit the round out in turns; ranked by points (1 for a win or
    /// for sitting out, 0.5 for a draw)
    MonsterDyp { rounds: NonZeroU32 },
}

impl PhasePreset {
    /// The format the phase is played in, which is what the preset configures the phase for.
    pub fn format(self) -> Box<dyn PhaseFormat> {
        match self {
            Self::RoundRobin => Box::new(RoundRobin),
            Self::SingleElimination {
                no_third_place_match: false,
            } => Box::new(SingleElimination::with_third_place_match()),
            Self::SingleElimination {
                no_third_place_match: true,
            } => Box::new(SingleElimination::new()),
            Self::DoubleElimination {
                no_grand_final_reset: false,
            } => Box::new(DoubleElimination::with_grand_final_reset()),
            Self::DoubleElimination {
                no_grand_final_reset: true,
            } => Box::new(DoubleElimination::new()),
            Self::RandomRounds { rounds } => Box::new(RandomRounds::new(rounds.get())),
            Self::MonsterDyp { rounds } => Box::new(MonsterDyp::new(rounds.get())),
        }
    }
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
                preset: Some(PhasePreset::RandomRounds {
                    rounds: NonZeroU32::new(5).unwrap()
                })
            }
        );
    }

    /// A knockout plays for third place unless the preset is told not to, which is the one thing
    /// about it that can be turned off without configuring the whole phase at the prompt.
    #[test]
    fn a_knockout_plays_for_third_place_unless_it_is_turned_off() {
        assert_eq!(
            parse("add-phase single-elimination"),
            Command::AddPhase {
                preset: Some(PhasePreset::SingleElimination {
                    no_third_place_match: false
                })
            }
        );
        assert_eq!(
            parse("add-phase single-elimination --no-third-place-match"),
            Command::AddPhase {
                preset: Some(PhasePreset::SingleElimination {
                    no_third_place_match: true
                })
            }
        );
    }

    #[test]
    fn a_double_elimination_resets_the_grand_final_unless_it_is_turned_off() {
        assert_eq!(
            parse("add-phase double-elimination"),
            Command::AddPhase {
                preset: Some(PhasePreset::DoubleElimination {
                    no_grand_final_reset: false
                })
            }
        );
        assert_eq!(
            parse("add-phase double-elimination --no-grand-final-reset"),
            Command::AddPhase {
                preset: Some(PhasePreset::DoubleElimination {
                    no_grand_final_reset: true
                })
            }
        );
    }

    /// A phase is configured at the prompt rather than taken from a preset, which is what the
    /// absent preset stands for.
    #[test]
    fn add_phase_takes_no_preset_at_all() {
        assert_eq!(parse("add-phase"), Command::AddPhase { preset: None });
    }

    #[test]
    fn a_phase_cannot_be_played_in_no_round() {
        assert!(Command::parse_line("add-phase random-rounds 0").is_err());
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
