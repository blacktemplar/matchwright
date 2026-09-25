//! Benchmarks a whole tournament with a large `RandomRounds` phase that calculates every metric
//! this crate offers.
//!
//! The number of entrants defaults to 1000 and can be configured through the `BENCH_ENTRANTS`
//! environment variable, which takes a comma separated list of entrant counts that are all
//! benchmarked separately, e.g. `BENCH_ENTRANTS=100,1000,2000 cargo bench`.
//!
//! The tournament itself is built by the shared [`tournament`] module, which the
//! `profile_tournament` example uses to run the exact same workload outside of criterion.

use std::{env, time::Duration};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

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

criterion_group!(benches, full_tournament);
criterion_main!(benches);
