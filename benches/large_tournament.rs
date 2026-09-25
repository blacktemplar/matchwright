//! Benchmarks a large [`RandomRounds`] phase with every metric this crate offers enabled.
//!
//! The number of entrants defaults to 1000 and can be configured through the `BENCH_ENTRANTS`
//! environment variable, which takes a comma separated list of entrant counts that are all
//! benchmarked separately, e.g. `BENCH_ENTRANTS=100,1000,2000 cargo bench`.

use std::{env, time::Duration};

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use itertools::Itertools;
use rand::{RngExt, SeedableRng, rngs::StdRng};
use tournament_engine::{
    entities::competition::{
        Competition, Entrant, EntrantId, MatchId, MatchResult, Phase, PhaseId,
    },
    plugins::{
        metrics_calculator::{
            buchholz::{
                Buchholz, BuchholzCalculator, BuchholzOfBuchholz, BuchholzOfBuchholzCalculator,
            },
            matches::MatchesCalculator,
            points::{Points, PointsCalculator},
            round_of_first_loss::RoundOfFirstLossCalculator,
            sonneborn_berger::{SonnebornBerger, SonnebornBergerCalculator},
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

type CompetitionStateMachine = StateMachine<Competition, Box<dyn Transition<Competition>>>;

/// The entrant counts benchmarked if `BENCH_ENTRANTS` is not set.
const DEFAULT_ENTRANTS: &str = "1000";

/// The number of rounds the benchmarked phase schedules.
const ROUNDS: u32 = 10;

/// Everything random in this benchmark is seeded, so that all runs benchmark the exact same
/// tournament.
const SEED: [u8; 12] = *b"bench-seed!!";

/// The scoring system of most association football leagues.
const THREE_ONE: PointsCalculator = PointsCalculator::new(3.0, 1.0);

/// The classical Sonneborn-Berger weights: the points of a beaten opponent count fully, those of
/// an opponent that was played to a draw count half.
const SONNEBORN_BERGER: SonnebornBergerCalculator = SonnebornBergerCalculator::new(1.0, 0.5);

fn entrant_counts() -> Vec<usize> {
    env::var("BENCH_ENTRANTS")
        .unwrap_or_else(|_| DEFAULT_ENTRANTS.to_owned())
        .split(',')
        .map(|count| {
            count
                .trim()
                .parse()
                .expect("BENCH_ENTRANTS must be a comma separated list of entrant counts")
        })
        .collect()
}

fn phase_id() -> PhaseId {
    PhaseId::new(Uuid::from_u128(0xfa5e))
}

fn apply(sm: &mut CompetitionStateMachine, transition: impl Transition<Competition> + 'static) {
    sm.apply(LogEntry::new(
        TransitionId::new(),
        LogEntryKind::Apply(Box::new(transition)),
    ))
    .expect("transition must be applicable");
}

/// A not yet started competition with `entrants` entrants and a single [`RandomRounds`] phase that
/// calculates every metric this crate offers and ranks by points, Buchholz, Sonneborn-Berger and
/// Buchholz of Buchholz.
fn unstarted_competition(entrants: usize) -> CompetitionStateMachine {
    let mut sm = CompetitionStateMachine::default();
    apply(&mut sm, SetCompetitionSeed::new(SEED));
    for entrant in 0..entrants {
        let id = EntrantId::new(Uuid::from_u128(entrant as u128));
        apply(&mut sm, AddEntrant::new(Entrant::new(id)));
    }
    let ranker = HierarchicalRanker::new(vec![Box::new(
        LexicographicalMetrics::new()
            .add_metric::<Points>()
            .add_metric::<Buchholz>()
            .add_metric::<SonnebornBerger>()
            .add_metric::<BuchholzOfBuchholz>(),
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
    sm
}

fn started_competition(entrants: usize) -> CompetitionStateMachine {
    let mut sm = unstarted_competition(entrants);
    apply(&mut sm, StartCompetition);
    sm
}

fn phase(sm: &CompetitionStateMachine) -> &Phase {
    &sm.current().phases()[&phase_id()]
}

/// The matches of the phase paired with the result they are reported with, ordered round by round.
///
/// Matches are ordered within a round by the initial rank of the first ranking of their team A,
/// which is unique and unaffected by any reported result, so that the schedule does not depend on
/// the iteration order of the match map.
fn reporting_schedule(sm: &CompetitionStateMachine) -> Vec<(MatchId, MatchResult)> {
    let phase = phase(sm);
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

/// Benchmarks starting the competition, which seeds the rankings, initializes all metrics and
/// creates the matches of all [`ROUNDS`] rounds at once.
fn start_competition(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_tournament/start_competition");
    for entrants in entrant_counts() {
        group.throughput(Throughput::Elements(entrants as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(entrants),
            &entrants,
            |b, &entrants| {
                b.iter_batched(
                    || unstarted_competition(entrants),
                    |mut sm| {
                        apply(&mut sm, StartCompetition);
                        sm
                    },
                    BatchSize::PerIteration,
                );
            },
        );
    }
    group.finish();
}

/// Benchmarks reporting the result of every match of the phase, which after every single result
/// updates all metrics and reranks the whole phase.
fn report_all_match_results(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_tournament/report_all_match_results");
    // a single iteration reports thousands of results, so a full sample size would take very long
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));
    for entrants in entrant_counts() {
        let num_matches = phase(&started_competition(entrants)).matches().len();
        group.throughput(Throughput::Elements(num_matches as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(entrants),
            &entrants,
            |b, &entrants| {
                b.iter_batched(
                    || {
                        let sm = started_competition(entrants);
                        let schedule = reporting_schedule(&sm);
                        (sm, schedule)
                    },
                    |(mut sm, schedule)| {
                        for (match_id, result) in schedule {
                            apply(
                                &mut sm,
                                ReportMatchResult::new(phase_id(), match_id, result),
                            );
                        }
                        sm
                    },
                    BatchSize::PerIteration,
                );
            },
        );
    }
    group.finish();
}

criterion_group!(benches, start_competition, report_all_match_results);
criterion_main!(benches);
