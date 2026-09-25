//! Runs a whole tournament and reports how long it took.
//!
//! The tournament is the one the `large_tournament` benchmark measures, but run as a plain binary
//! without a harness around it:
//!
//! ```text
//! cargo run --release --example run_tournament -- 1000
//! ```
//!
//! The number of entrants is taken from the command line and defaults to 1000.

use std::{env, hint::black_box, time::Instant};

#[path = "../benches/common/tournament.rs"]
mod tournament;

fn main() {
    let entrants = env::args()
        .nth(1)
        .map_or(tournament::DEFAULT_ENTRANTS, |entrants| {
            entrants
                .trim()
                .parse()
                .expect("the number of entrants must be a number")
        });

    let start = Instant::now();
    let sm = black_box(tournament::run_tournament(entrants));
    let elapsed = start.elapsed();
    // the finished competition is huge, so it is dropped after the time is taken
    let matches = sm.current().phases()[&tournament::phase_id()]
        .matches()
        .len();
    println!("{entrants} entrants: {matches} matches reported in {elapsed:.2?}");
    drop(sm);
}
