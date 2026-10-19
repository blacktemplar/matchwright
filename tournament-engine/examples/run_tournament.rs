//! Runs a whole tournament and reports how long it took.
//!
//! The tournament is the one the `large_tournament` benchmark measures, but run as a plain binary
//! without a harness around it:
//!
//! ```text
//! cargo run --release --example run_tournament -- 1000 --postcard /tmp/tournament.postcard
//! ```
//!
//! The number of entrants is taken from the command line and defaults to 1000. If `--json` or
//! `--postcard` is given, the finished competition is additionally serialized to the path that
//! follows the option.

use std::{
    env,
    fs::File,
    hint::black_box,
    io::{BufWriter, Write},
    path::PathBuf,
    time::Instant,
};

use tournament_engine::Competition;

#[path = "../benches/common/tournament.rs"]
mod tournament;

/// Where the finished competition is written to, if anywhere.
struct OutputPaths {
    json: Option<PathBuf>,
    postcard: Option<PathBuf>,
}

/// # Panics
/// Panics if the number of entrants is not a number, if an unknown option is given or if `--json`
/// or `--postcard` is given without a path.
fn parse_args() -> (usize, OutputPaths) {
    let mut entrants = tournament::DEFAULT_ENTRANTS;
    let mut paths = OutputPaths {
        json: None,
        postcard: None,
    };
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut path = |option| {
            args.next()
                .map(PathBuf::from)
                .unwrap_or_else(|| panic!("{option} must be followed by a path"))
        };
        match arg.as_str() {
            "--json" => paths.json = Some(path("--json")),
            "--postcard" => paths.postcard = Some(path("--postcard")),
            _ if arg.starts_with('-') => panic!("unknown option {arg}"),
            _ => {
                entrants = arg
                    .trim()
                    .parse()
                    .expect("the number of entrants must be a number");
            }
        }
    }
    (entrants, paths)
}

/// Serializes `competition` with `serialize` and writes the result to `path`, reporting how long
/// that took.
///
/// # Panics
/// Panics if serializing fails or if `path` cannot be written.
fn write(
    path: &PathBuf,
    format: &str,
    competition: &Competition,
    serialize: impl FnOnce(&Competition) -> Vec<u8>,
) {
    let start = Instant::now();
    let bytes = serialize(competition);
    let mut file = BufWriter::new(
        File::create(path)
            .unwrap_or_else(|error| panic!("{} cannot be written: {error}", path.display())),
    );
    file.write_all(&bytes)
        .and_then(|()| file.flush())
        .unwrap_or_else(|error| panic!("{} cannot be written: {error}", path.display()));
    let elapsed = start.elapsed();
    println!(
        "{format}: {} bytes written to {} in {elapsed:.2?}",
        bytes.len(),
        path.display()
    );
}

fn main() {
    let (entrants, paths) = parse_args();

    let start = Instant::now();
    let sm = black_box(tournament::run_tournament(entrants));
    let elapsed = start.elapsed();
    // the finished competition is huge, so it is dropped after the time is taken
    let matches = sm.current().phases()[&tournament::phase_id()]
        .data()
        .matches()
        .len();
    println!("{entrants} entrants: {matches} matches reported in {elapsed:.2?}");

    if let Some(path) = &paths.postcard {
        write(path, "postcard", sm.current(), |competition| {
            postcard::to_stdvec(competition).expect("serializing a competition must not fail")
        });
    }
    if let Some(path) = &paths.json {
        write(path, "json", sm.current(), |competition| {
            serde_json::to_vec(competition).expect("serializing a competition must not fail")
        });
    }

    drop(sm);
}
