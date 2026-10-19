//! Builds and plays through a whole tournament with a large [`RandomRounds`] phase that calculates
//! every metric this crate offers.
//!
//! This module is shared between the `large_tournament` benchmark and the `run_tournament`
//! example, which both include it through `#[path = "..."] mod tournament;`.

use std::sync::LazyLock;

use itertools::Itertools;
use rand::{RngExt, SeedableRng, rngs::StdRng};
use tournament_engine::{
    entities::competition::{Competition, Entrant, EntrantId, MatchId, MatchResult, PhaseId},
    plugins::{
        PhasePlugins,
        metrics_calculator::{
            buchholz::{
                BuchholzCalculator, BuchholzOfBuchholzCalculator, MoreBuchholz,
                MoreBuchholzOfBuchholz, MoreSonnebornBerger, SonnebornBergerCalculator,
            },
            matches::MatchesCalculator,
            points::{MorePoints, PointsCalculator},
            round_of_first_loss::RoundOfFirstLossCalculator,
            wins_draws_losses::WinsDrawsLossesCalculator,
        },
        phase_format::round_robin::random_rounds::RandomRounds,
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

/// The competition seed, the pairings and the reported results are seeded, so that a run only
/// differs from another one in its randomly generated ids.
const SEED: [u8; 12] = *b"bench-seed!!";

/// The scoring system of most association football leagues.
const THREE_ONE: PointsCalculator = PointsCalculator::new(3.0, 1.0);

/// The classical Sonneborn-Berger weights: the points of a beaten opponent count fully, those of
/// an opponent that was played to a draw count half.
const SONNEBORN_BERGER: SonnebornBergerCalculator = SonnebornBergerCalculator::new(1.0, 0.5);

/// The id of the single phase every tournament of this module consists of.
///
/// It is generated once per process, so that all tournaments a single run plays share it.
pub fn phase_id() -> PhaseId {
    static PHASE_ID: LazyLock<PhaseId> = LazyLock::new(|| PhaseId::new(Uuid::new_v4()));
    *PHASE_ID
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
    for _ in 0..entrants {
        let id = EntrantId::new(Uuid::new_v4());
        apply(&mut sm, AddEntrant::new(Entrant::new(id)));
    }
    apply(
        &mut sm,
        AddPhase::new(
            phase_id(),
            PhasePlugins::new(
                Box::new(RandomRounds::new(ROUNDS)),
                vec![
                    Box::new(MorePoints),
                    Box::new(MoreBuchholz),
                    Box::new(MoreSonnebornBerger),
                    Box::new(MoreBuchholzOfBuchholz),
                ],
                vec![
                    Box::new(RoundOfFirstLossCalculator),
                    Box::new(WinsDrawsLossesCalculator),
                    Box::new(THREE_ONE),
                    Box::new(MatchesCalculator),
                    Box::new(BuchholzCalculator),
                    Box::new(SONNEBORN_BERGER),
                    Box::new(BuchholzOfBuchholzCalculator),
                ],
            )
            .unwrap(),
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
