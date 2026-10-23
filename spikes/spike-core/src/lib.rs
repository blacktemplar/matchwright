//! What both GUI spikes show and do, so that they differ only in the framework.
//!
//! A [`Tournament`] runs one competition with one phase on the engine, much like the CLI's session,
//! but hands out plain data rather than text, for a GUI to lay out.

use std::collections::HashMap;

use plugin_factory::{
    catalog::AnyAxis,
    config::{Path as ConfigPath, PluginConfig},
};
use tournament_cli::questionnaire::{self, AcceptDefaults};
use tournament_engine::{
    AddEntrant, AddPhase, CompetitionStateMachine, CompetitionStateMachineExt as _, Entrant,
    EntrantId, Phase, PhaseData, PhaseId, PhasePluginsBuilder, RankingId, ReportMatchResult, Score,
    Seed, SetCompetitionSeed, StartCompetition,
    plugins::{
        metrics_calculator::{
            BuchholzCalculator, BuchholzOfBuchholzCalculator, MoreBuchholz, MoreBuchholzOfBuchholz,
            MorePoints, MoreSonnebornBerger, PointsCalculator, RoundOfFirstLossCalculator,
            SonnebornBergerCalculator, WinsDrawsLossesCalculator,
        },
        phase_format::{PhaseFormat, RandomRounds},
    },
    undoable_state_machine::SessionHistory,
    uuid::Uuid,
};
pub use tournament_engine::{MatchId, MatchResult};

/// A format a phase can be played in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    pub id: &'static str,
    pub label: &'static str,
}

/// A match as a GUI lists it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MatchRow {
    pub id: MatchId,
    /// The position the match is listed at, from 1.
    pub number: usize,
    pub round: u32,
    pub stage: Option<String>,
    pub a: String,
    pub b: String,
    pub result: Option<MatchResult>,
}

/// One line of the standings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StandingRow {
    pub rank: u32,
    pub name: String,
    /// One cell per column of [`Standings::columns`].
    pub cells: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Standings {
    /// The headers of the metric columns, after rank and name.
    pub columns: Vec<&'static str>,
    pub rows: Vec<StandingRow>,
}

/// A competition with one phase, run from a GUI. Every method that changes it returns a message
/// for the operator if the change is rejected.
pub struct Tournament {
    machine: CompetitionStateMachine,
    history: SessionHistory,
    names: HashMap<EntrantId, String>,
}

impl Default for Tournament {
    fn default() -> Self {
        Self::new()
    }
}

impl Tournament {
    /// A new competition with a random seed.
    pub fn new() -> Self {
        let mut seed = [0; 12];
        getrandom::fill(&mut seed).expect("the platform must provide randomness");
        let mut machine = CompetitionStateMachine::default();
        machine
            .apply_transition(SetCompetitionSeed::new(Seed::new(seed)))
            .expect("a new competition must accept a seed");
        Self {
            machine,
            history: SessionHistory::new(),
            names: HashMap::new(),
        }
    }

    /// The competition of the engine's `large_tournament` benchmark: `entrants` entrants, one
    /// random rounds phase of 10 rounds that calculates every metric and ranks by points,
    /// Buchholz, Sonneborn-Berger and Buchholz of Buchholz, started, and with the first `reported`
    /// matches reported. None of it can be undone.
    pub fn large(entrants: usize, reported: usize) -> Self {
        let mut tournament = Self::new();
        for number in 1..=entrants {
            let id = EntrantId::new(Uuid::new_v4());
            tournament
                .machine
                .apply_transition(AddEntrant::new(Entrant::new(id)))
                .expect("entrants are added before the start");
            tournament.names.insert(id, format!("Player {number}"));
        }
        let plugins = PhasePluginsBuilder::new()
            .format(RandomRounds::new(10))
            .add_ranking(MorePoints)
            .add_ranking(MoreBuchholz)
            .add_ranking(MoreSonnebornBerger)
            .add_ranking(MoreBuchholzOfBuchholz)
            .add_metric(RoundOfFirstLossCalculator)
            .add_metric(WinsDrawsLossesCalculator)
            .add_metric(PointsCalculator::new(Score::new(3.0), Score::ONE))
            .add_metric(BuchholzCalculator)
            .add_metric(SonnebornBergerCalculator::new(Score::ONE, Score::new(0.5)))
            .add_metric(BuchholzOfBuchholzCalculator)
            .build()
            .expect("the benchmark's plugins fit together");
        tournament
            .machine
            .apply_transition(AddPhase::new(PhaseId::new(Uuid::new_v4()), plugins))
            .expect("the phase is added before the start");
        tournament
            .machine
            .apply_transition(StartCompetition)
            .expect("the competition can start");
        for index in 0..reported {
            let id = tournament
                .first_open_match()
                .expect("there are enough matches to report");
            let phase_id = tournament.started_phase().expect("started").id();
            // 40% won by A, 40% by B, 20% drawn, as in the benchmark
            let result = match index % 5 {
                0 | 1 => MatchResult::AWins,
                2 | 3 => MatchResult::BWins,
                _ => MatchResult::Draw,
            };
            tournament
                .machine
                .apply_transition(ReportMatchResult::new(phase_id, id, result))
                .expect("an open match takes a result");
        }
        tournament
    }

    /// The log of the competition as JSON, without the names.
    pub fn log_json(&self) -> String {
        serde_json::to_string(&self.machine).expect("a log serializes")
    }

    /// The competition `json`, a [`Self::log_json`], replayed. Its entrants have no names.
    pub fn from_log_json(json: &str) -> Result<Self, String> {
        Ok(Self {
            machine: serde_json::from_str(json).map_err(|error| error.to_string())?,
            history: SessionHistory::new(),
            names: HashMap::new(),
        })
    }

    /// The first match that has no result yet.
    pub fn first_open_match(&self) -> Option<MatchId> {
        self.started_phase()?
            .data()
            .matches()
            .values()
            .find(|r#match| r#match.result().is_none())
            .map(|r#match| r#match.id())
    }

    /// Every registered phase format.
    pub fn formats() -> Vec<Format> {
        AnyAxis::of::<dyn PhaseFormat>()
            .entries()
            .map(|entry| Format {
                id: entry.id(),
                label: entry.label(),
            })
            .collect()
    }

    /// The names of the entrants, in the order they were added.
    pub fn entrants(&self) -> Vec<String> {
        self.machine
            .current()
            .entrants()
            .keys()
            .map(|id| self.name_of(id).to_owned())
            .collect()
    }

    pub fn add_entrant(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("an entrant name must not be empty".to_owned());
        }
        if self.names.values().any(|existing| existing == name) {
            return Err(format!("there already is an entrant named {name:?}"));
        }
        let id = EntrantId::new(Uuid::new_v4());
        self.machine
            .apply_in(&mut self.history, AddEntrant::new(Entrant::new(id)))
            .map_err(|error| error.to_string())?;
        self.names.insert(id, name.to_owned());
        Ok(())
    }

    /// Adds eight entrants with made-up names.
    pub fn add_demo_entrants(&mut self) -> Result<(), String> {
        let first = self.names.len() + 1;
        for number in first..first + 8 {
            self.add_entrant(&format!("Player {number}"))?;
        }
        Ok(())
    }

    pub fn has_phase(&self) -> bool {
        self.phase().is_some()
    }

    /// Adds the phase, played in the format `format_id` with every field answered as offered.
    pub fn add_phase(&mut self, format_id: &str) -> Result<(), String> {
        let mut draft = self.machine.current().phase_draft();
        draft
            .set(
                &ConfigPath::ROOT.field("format"),
                PluginConfig::new(format_id),
            )
            .map_err(|error| error.to_string())?;
        questionnaire::fill(&mut draft, &mut AcceptDefaults).map_err(|error| error.to_string())?;
        draft.add_implied();
        let plugins = draft.build().map_err(|issues| {
            issues
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        })?;
        self.machine
            .apply_in(
                &mut self.history,
                AddPhase::new(PhaseId::new(Uuid::new_v4()), plugins),
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn is_started(&self) -> bool {
        self.machine.current().is_started()
    }

    pub fn start(&mut self) -> Result<(), String> {
        self.machine
            .apply_in(&mut self.history, StartCompetition)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    /// The matches of the phase, empty before the start.
    pub fn matches(&self) -> Vec<MatchRow> {
        let Some(phase) = self.started_phase() else {
            return Vec::new();
        };
        let data = phase.data();
        data.matches()
            .values()
            .enumerate()
            .map(|(index, r#match)| MatchRow {
                id: r#match.id(),
                number: index + 1,
                round: r#match.round().get(),
                stage: phase.format().stage(r#match),
                a: self.team_name(data, r#match.team_a()),
                b: self.team_name(data, r#match.team_b()),
                result: r#match.result(),
            })
            .collect()
    }

    pub fn report(&mut self, id: MatchId, result: MatchResult) -> Result<(), String> {
        let phase_id = self.started_phase().ok_or("not started")?.id();
        self.machine
            .apply_in(
                &mut self.history,
                ReportMatchResult::new(phase_id, id, result),
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    /// Undoes the result of the match `id`, whether it was reported last or not.
    pub fn undo_report(&mut self, id: MatchId) -> Result<(), String> {
        let phase_id = self.started_phase().ok_or("not started")?.id();
        self.machine
            .undo_match_result(&mut self.history, phase_id, id)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn standings(&self) -> Standings {
        let Some(phase) = self.started_phase() else {
            return Standings::default();
        };
        let columns: Vec<_> = phase
            .metrics_calculators()
            .iter()
            .filter_map(|calculator| calculator.column())
            .collect();
        let data = phase.data();
        Standings {
            columns: columns.iter().map(|column| column.header()).collect(),
            rows: data
                .standings()
                .map(|ranking| StandingRow {
                    rank: ranking.rank().get(),
                    name: self.team_name(data, &[ranking.id()]),
                    cells: columns
                        .iter()
                        .map(|column| column.cell(ranking).unwrap_or_default())
                        .collect(),
                })
                .collect(),
        }
    }

    pub fn undo(&mut self) -> Result<(), String> {
        self.history
            .undo_last(&mut self.machine)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn redo(&mut self) -> Result<(), String> {
        self.history
            .redo_last(&mut self.machine)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn phase(&self) -> Option<&Phase> {
        self.machine.current().phases().values().next()
    }

    fn started_phase(&self) -> Option<&Phase> {
        self.phase().filter(|_| self.is_started())
    }

    fn name_of(&self, id: &EntrantId) -> &str {
        self.names.get(id).map_or("<unnamed>", String::as_str)
    }

    fn team_name(&self, phase: &PhaseData, rankings: &[RankingId]) -> String {
        rankings
            .iter()
            .flat_map(|id| phase.rankings()[id].entrants())
            .map(|id| self.name_of(id))
            .collect::<Vec<_>>()
            .join(" & ")
    }
}
