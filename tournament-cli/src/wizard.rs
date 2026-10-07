//! Configuring a phase at the prompt, question by question.
//!
//! Every question starts out at the answer the format comes with, see
//! [`PhaseConfig::with_defaults`], so accepting all of them with Enter configures the same phase
//! the preset of that format does. Cancelling a question with Esc cancels the whole configuration.
//!
//! This is the only part of the CLI that reads from the terminal on its own. The configured phase
//! is handed to [`Session::add_phase`](crate::session::Session::add_phase), which is what changes
//! the competition.

use std::iter;

use inquire::{Confirm, CustomType, MultiSelect, Select, error::InquireResult};

use crate::phase::{Criterion, FormatConfig, MetricConfig, MetricKind, PhaseConfig};

/// What the ranking questions offer instead of a further metric, which ends them.
const NO_FURTHER_METRIC: &str = "(no further metric)";

/// Asks for every part of a phase and returns what was configured.
///
/// # Errors
/// Returns [`InquireError::OperationCanceled`] if the operator cancels a question with Esc,
/// [`InquireError::OperationInterrupted`] on Ctrl-C and [`InquireError::NotTTY`] if there is no
/// terminal to ask at, e.g. because the input is piped in.
///
/// [`InquireError::OperationCanceled`]: inquire::InquireError::OperationCanceled
/// [`InquireError::OperationInterrupted`]: inquire::InquireError::OperationInterrupted
/// [`InquireError::NotTTY`]: inquire::InquireError::NotTTY
pub fn configure_phase() -> InquireResult<PhaseConfig> {
    let format = ask_format()?;
    let metrics = ask_metrics(format)?;
    let ranked_by = ask_ranking(format, &metrics)?;
    Ok(PhaseConfig::new(format, metrics, ranked_by))
}

/// How the option for `label` is shown, with `description` telling what it does.
fn option(label: &str, description: &str) -> String {
    format!("{label} — {description}")
}

/// Asks for the format and, if it has parameters, for those too.
fn ask_format() -> InquireResult<FormatConfig> {
    let options: Vec<_> = FormatConfig::ALL
        .iter()
        .map(|format| option(format.label(), format.description()))
        .collect();
    // the chosen index is the one into `FormatConfig::ALL`, which is what carries the parameters
    // the format is offered with
    let chosen = Select::new("Phase format", options).raw_prompt()?;
    match FormatConfig::ALL[chosen.index] {
        FormatConfig::RandomRounds { rounds } => Ok(FormatConfig::RandomRounds {
            rounds: CustomType::new("How many rounds?")
                .with_default(rounds)
                .with_help_message(
                    "a full round robin is scheduled if that is shorter than the number asked for",
                )
                .with_error_message("the phase has to be played in at least one round")
                .prompt()?,
        }),
        FormatConfig::SingleElimination { third_place_match } => {
            Ok(FormatConfig::SingleElimination {
                third_place_match: Confirm::new("Play a match for third place?")
                    .with_default(third_place_match)
                    .with_help_message(
                        "the two entrants knocked out in the semifinals play it alongside the \
                         final; without it they stay third equal",
                    )
                    .prompt()?,
            })
        }
        format => Ok(format),
    }
}

/// Asks which metrics are calculated, and for the parameters of each of those that has any.
fn ask_metrics(format: FormatConfig) -> InquireResult<Vec<MetricConfig>> {
    let options: Vec<_> = MetricKind::SELECTABLE
        .iter()
        .map(|kind| option(kind.label(), kind.description()))
        .collect();
    let defaults: Vec<_> = MetricKind::SELECTABLE
        .iter()
        .enumerate()
        .filter(|(_, kind)| format.default_metrics().contains(kind))
        .map(|(index, _)| index)
        .collect();
    let chosen = MultiSelect::new("Metrics to calculate", options)
        .with_default(&defaults)
        .with_help_message(
            "space toggles a metric, enter confirms; every metric a chosen one is derived from is \
             calculated as well",
        )
        .raw_prompt()?;
    chosen
        .iter()
        .map(|chosen| ask_parameters(MetricKind::SELECTABLE[chosen.index]))
        .collect()
}

/// Asks for the parameters of `kind`, which is answered for it if it has none.
fn ask_parameters(kind: MetricKind) -> InquireResult<MetricConfig> {
    match kind.with_defaults() {
        MetricConfig::Points { for_win, for_draw } => Ok(MetricConfig::Points {
            for_win: CustomType::new("Points for a win")
                .with_default(for_win)
                .prompt()?,
            for_draw: CustomType::new("Points for a draw")
                .with_default(for_draw)
                .prompt()?,
        }),
        MetricConfig::SonnebornBerger {
            weight_for_win,
            weight_for_draw,
        } => Ok(MetricConfig::SonnebornBerger {
            weight_for_win: CustomType::new("Sonneborn-Berger: how much a beaten opponent counts")
                .with_default(weight_for_win)
                .with_help_message("the points of that opponent are multiplied by this")
                .prompt()?,
            weight_for_draw: CustomType::new(
                "Sonneborn-Berger: how much an opponent played to a draw counts",
            )
            .with_default(weight_for_draw)
            .with_help_message("the points of that opponent are multiplied by this")
            .prompt()?,
        }),
        metric => Ok(metric),
    }
}

/// Asks which of the calculated `metrics` rank the entrants, one question per metric, until the
/// operator asks for no further one or no metric is left to ask for.
fn ask_ranking(format: FormatConfig, metrics: &[MetricConfig]) -> InquireResult<Vec<Criterion>> {
    let mut remaining: Vec<_> = metrics
        .iter()
        .flat_map(|metric| metric.kind().criteria().iter().copied())
        .collect();
    let mut ranked_by = Vec::new();
    while !remaining.is_empty() {
        let options: Vec<_> = remaining
            .iter()
            .map(|metric| metric.label().to_owned())
            .chain(iter::once(NO_FURTHER_METRIC.to_owned()))
            .collect();
        // the cursor starts at the metric the format would rank by next, so that accepting every
        // question with enter gives the ranking of its preset; every metric already ranked by is
        // gone from `remaining`, and so is out of the running on its own
        let cursor = format
            .default_ranked_by()
            .iter()
            .find_map(|next| remaining.iter().position(|metric| metric == next))
            .unwrap_or(remaining.len());
        let question = if ranked_by.is_empty() {
            "Rank by"
        } else {
            "Then by"
        };
        let chosen = Select::new(question, options)
            .with_starting_cursor(cursor)
            .with_help_message("every further metric only breaks the ties of the ones before it")
            .raw_prompt()?;
        if chosen.index == remaining.len() {
            break;
        }
        ranked_by.push(remaining.remove(chosen.index));
    }
    Ok(ranked_by)
}
