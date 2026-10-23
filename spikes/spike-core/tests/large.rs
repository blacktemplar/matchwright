//! Times what an operator waits for with the competition of the engine's `large_tournament`
//! benchmark, without any UI. Not a test of anything, run it with its output shown:
//!
//! ```sh
//! cargo test --release -p spike-core --test large -- --nocapture
//! wasm-pack test --node --release spike-core --test large -- --nocapture
//! ```

use spike_core::{MatchResult, Tournament};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;
use web_time::Instant;

const ENTRANTS: usize = 1000;

fn print(line: &str) {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("{line}");
    #[cfg(not(target_arch = "wasm32"))]
    println!("{line}");
}

/// Runs `f`, printing how long it took.
fn timed<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = f();
    print(&format!(
        "  {what:<44} {:>9.1} ms",
        start.elapsed().as_secs_f64() * 1000.0
    ));
    result
}

#[test]
fn large_tournament() {
    // 10 rounds of 500 matches each
    for reported in [0, 2500, 4999] {
        print(&format!(
            "{ENTRANTS} entrants, {reported} of 5000 results reported:"
        ));
        let mut tournament = timed("build (add, start, report)", || {
            Tournament::large(ENTRANTS, reported)
        });
        timed("matches view (5000 rows)", || tournament.matches());
        timed("standings view (1000 rows)", || tournament.standings());
        let id = tournament.first_open_match().unwrap();
        timed("report one result", || {
            tournament.report(id, MatchResult::AWins).unwrap();
        });
        timed("undo it (replays the log)", || tournament.undo().unwrap());
        timed("redo it (replays the log)", || tournament.redo().unwrap());
        timed("undo the result by match (replays)", || {
            tournament.undo_report(id).unwrap();
        });
        let json = timed("save the log as JSON", || tournament.log_json());
        print(&format!(
            "  {:<44} {:>9.1} KB",
            "  size",
            json.len() as f64 / 1024.0
        ));
        timed("load it (deserialize and replay)", || {
            Tournament::from_log_json(&json).unwrap()
        });
    }
}
