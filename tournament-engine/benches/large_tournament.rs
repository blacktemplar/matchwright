//! Benchmarks a whole tournament with a large `RandomRounds` phase that calculates every metric
//! this crate offers, and a round trip of the finished competition through both serialization
//! formats.
//!
//! The number of entrants defaults to 1000 and can be configured through the `BENCH_ENTRANTS`
//! environment variable, which takes a comma separated list of entrant counts that are all
//! benchmarked separately, e.g. `BENCH_ENTRANTS=100,1000,2000 cargo bench`.
//!
//! The tournament itself is built by the shared [`tournament`] module, which the
//! `profile_tournament` example uses to run the exact same workload outside of criterion.

use std::{env, time::Duration};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use tournament_engine::entities::competition::Competition;

use crate::tournament::DEFAULT_ENTRANTS;

#[path = "common/tournament.rs"]
mod tournament;

/// The entrant counts configured through the `BENCH_ENTRANTS` environment variable, which takes a
/// comma separated list of entrant counts, e.g. `BENCH_ENTRANTS=100,1000,2000`.
pub fn entrant_counts() -> Vec<usize> {
    let Ok(counts) = env::var("BENCH_ENTRANTS") else {
        return vec![DEFAULT_ENTRANTS];
    };
    counts
        .split(',')
        .map(|count| {
            count
                .trim()
                .parse()
                .expect("BENCH_ENTRANTS must be a comma separated list of entrant counts")
        })
        .collect()
}

fn full_tournament(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_tournament");
    // a single iteration reports thousands of match results, so a full sample size would take very
    // long
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));
    for entrants in entrant_counts() {
        group.bench_with_input(
            BenchmarkId::from_parameter(entrants),
            &entrants,
            // the finished competition is huge, so it is dropped outside of the measurement
            |b, &entrants| b.iter_with_large_drop(|| tournament::run_tournament(entrants)),
        );
    }
    group.finish();
}

/// Benchmarks serializing the competition a whole tournament leaves behind and deserializing it
/// again, once through [`postcard`] as a compact binary format and once through [`serde_json`] as a
/// text one.
///
/// Playing the tournament happens once per entrant count, outside of the measurement, so that only
/// the round trip itself is measured.
fn serialization_round_trip(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization_round_trip");
    for entrants in entrant_counts() {
        let sm = tournament::run_tournament(entrants);
        let competition = sm.current();

        group.bench_with_input(
            BenchmarkId::new("postcard", entrants),
            competition,
            // the deserialized competition is huge, so it is dropped outside of the measurement
            |b, competition| {
                b.iter_with_large_drop(|| {
                    let bytes = postcard::to_stdvec(competition)
                        .expect("serializing a competition must not fail");
                    postcard::from_bytes::<Competition>(&bytes)
                        .expect("a serialized competition must deserialize again")
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("json", entrants),
            competition,
            |b, competition| {
                b.iter_with_large_drop(|| {
                    let json = serde_json::to_vec(competition)
                        .expect("serializing a competition must not fail");
                    serde_json::from_slice::<Competition>(&json)
                        .expect("a serialized competition must deserialize again")
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, full_tournament, serialization_round_trip);
criterion_main!(benches);
