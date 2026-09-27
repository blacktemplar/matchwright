//! Builds and plays through a whole tournament with a large [`RandomRounds`] phase that calculates
//! every metric this crate offers.
//!
//! This module is shared between the `large_tournament` benchmark and the `run_tournament`
//! example, which both include it through `#[path = "..."] mod tournament;`.

use itertools::Itertools;
use rand::{RngExt, SeedableRng, rngs::StdRng};
use tournament_engine::{
    entities::competition::{Competition, Entrant, EntrantId, MatchId, MatchResult, PhaseId},
    plugins::{
        metrics_calculator::{
            buchholz::{
                BuchholzCalculator, BuchholzGetter, BuchholzOfBuchholzCalculator,
                BuchholzOfBuchholzGetter,
            },
            matches::MatchesCalculator,
            points::{PointsCalculator, PointsGetter},
            round_of_first_loss::RoundOfFirstLossCalculator,
            sonneborn_berger::{SonnebornBergerCalculator, SonnebornBergerGetter},
            wins_draws_losses::WinsDrawsLossesCalculator,
        },
        phase_format::random_rounds::RandomRounds,
        ranker::{HierarchicalRanker, LexicographicalMetrics},
    },
    transitions::{
        add_entrant::AddEntrant, add_phase::AddPhase, report_match_result::ReportMatchResult,
        set_competition_seed::SetCompetitionSeed, start_competition::StartCompetition,
    },
};
use undoable_state_machine::{LogEntry, LogEntryKind, StateMachine, Transition, TransitionId};
use uuid::Uuid;

pub type CompetitionStateMachine = StateMachine<Competition, Box<dyn Transition<Competition>>>;

/// The number of entrants used if nothing else is configured.
pub const DEFAULT_ENTRANTS: usize = 1000;

/// The number of rounds the phase schedules.
const ROUNDS: u32 = 10;

/// Everything random here is seeded, so that all runs use the exact same tournament.
const SEED: [u8; 12] = *b"bench-seed!!";

/// The scoring system of most association football leagues.
const THREE_ONE: PointsCalculator = PointsCalculator::new(3.0, 1.0);

/// The classical Sonneborn-Berger weights: the points of a beaten opponent count fully, those of
/// an opponent that was played to a draw count half.
const SONNEBORN_BERGER: SonnebornBergerCalculator = SonnebornBergerCalculator::new(1.0, 0.5);

pub fn phase_id() -> PhaseId {
    PhaseId::new(Uuid::from_u128(0xfa5e))
}

fn apply(sm: &mut CompetitionStateMachine, transition: impl Transition<Competition> + 'static) {
    sm.apply(LogEntry::new(
        TransitionId::new(),
        LogEntryKind::Apply(Box::new(transition)),
    ))
    .expect("transition must be applicable");
}

/// The matches of the phase paired with the result they are reported with, ordered round by round.
///
/// Matches are ordered within a round by the initial rank of the first ranking of their team A,
/// which is unique and unaffected by any reported result, so that the schedule does not depend on
/// the iteration order of the match map.
fn reporting_schedule(sm: &CompetitionStateMachine) -> Vec<(MatchId, MatchResult)> {
    let phase = &sm.current().phases()[&phase_id()];
    let mut rng = StdRng::seed_from_u64(0x7e57_5eed);
    phase
        .matches()
        .values()
        .sorted_by_key(|r#match| {
            let first_ranking = &phase.rankings()[&r#match.team_a()[0]];
            (r#match.round(), first_ranking.initial_unique_rank())
        })
        .map(|r#match| {
            // 40% of all matches are won by team A, 40% by team B and the remaining 20% end in a
            // draw
            let result = match rng.random_range(0..10u32) {
                0..4 => MatchResult::AWins,
                4..8 => MatchResult::BWins,
                _ => MatchResult::Draw,
            };
            (r#match.id(), result)
        })
        .collect()
}

/// Runs a whole tournament: adds `entrants` entrants and a single [`RandomRounds`] phase that
/// calculates every metric this crate offers and ranks by points, Buchholz, Sonneborn-Berger and
/// Buchholz of Buchholz, starts the competition and finally reports the result of every match.
pub fn run_tournament(entrants: usize) -> CompetitionStateMachine {
    let mut sm = CompetitionStateMachine::default();
    apply(&mut sm, SetCompetitionSeed::new(SEED));
    for entrant in 0..entrants {
        let id = EntrantId::new(Uuid::from_u128(entrant as u128));
        apply(&mut sm, AddEntrant::new(Entrant::new(id)));
    }
    let ranker = HierarchicalRanker::new(vec![Box::new(
        LexicographicalMetrics::new()
            .add_metric(PointsGetter)
            .add_metric(BuchholzGetter)
            .add_metric(SonnebornBergerGetter)
            .add_metric(BuchholzOfBuchholzGetter),
    )]);
    apply(
        &mut sm,
        AddPhase::new(
            phase_id(),
            Box::new(RandomRounds::new(ROUNDS)),
            Box::new(ranker),
            // the calculators run in this order, so every one of them that reads the metric of
            // another one has to be listed after it
            vec![
                Box::new(RoundOfFirstLossCalculator),
                Box::new(WinsDrawsLossesCalculator),
                Box::new(THREE_ONE),
                Box::new(MatchesCalculator),
                Box::new(BuchholzCalculator),
                Box::new(SONNEBORN_BERGER),
                Box::new(BuchholzOfBuchholzCalculator),
            ],
        ),
    );
    // this creates the rankings, initializes all metrics and schedules the matches of all rounds
    apply(&mut sm, StartCompetition);
    // every single result updates all metrics and reranks the whole phase
    for (match_id, result) in reporting_schedule(&sm) {
        apply(
            &mut sm,
            ReportMatchResult::new(phase_id(), match_id, result),
        );
    }
    sm
}
