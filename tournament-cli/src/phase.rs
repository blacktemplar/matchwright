//! The phase a competition is played in, as an operator configures it.
//!
//! A [`PhaseConfig`] holds everything the CLI can decide about that phase: the format its matches
//! are scheduled by, the metrics calculated for every entrant, and the order in which those
//! metrics rank them. Both a preset of `add-phase` and configuring a phase at the prompt (see
//! [`crate::wizard`]) produce one, which [`Session::add_phase`] turns into the plugins of the
//! engine.
//!
//! [`Session::add_phase`]: crate::session::Session::add_phase

use std::num::NonZeroU32;

use tournament_engine::plugins::{
    metrics_calculator::{
        MetricsCalculator,
        buchholz::{
            BuchholzCalculator, BuchholzOfBuchholzCalculator, MoreBuchholz, MoreBuchholzOfBuchholz,
        },
        matches::MatchesCalculator,
        points::{MorePoints, PointsCalculator},
        round_of_elimination::{LaterElimination, RoundOfEliminationCalculator},
        round_of_first_loss::{LaterFirstLoss, RoundOfFirstLossCalculator},
        sonneborn_berger::{MoreSonnebornBerger, SonnebornBergerCalculator},
        wins_draws_losses::{
            FewerLosses, FewerMatchesPlayed, MoreDraws, MoreWins, WinsDrawsLossesCalculator,
        },
    },
    phase_format::{
        PhaseFormat, double_elimination::DoubleElimination, monster_dyp::MonsterDyp,
        random_rounds::RandomRounds, round_robin::RoundRobin,
        single_elimination::SingleElimination,
    },
    ranker::{HierarchicalRanker, LexicographicalCriteria, Ranker},
};

use crate::command::PhasePreset;

/// The number of rounds the random rounds format is offered with.
const DEFAULT_RANDOM_ROUNDS: NonZeroU32 = NonZeroU32::new(5).unwrap();

/// The number of rounds the Monster DYP format is offered with.
const DEFAULT_MONSTER_DYP_ROUNDS: NonZeroU32 = NonZeroU32::new(5).unwrap();

/// The scoring system of chess and most other games scoring a draw as half a win, with a bye worth
/// as much as a win.
const DEFAULT_POINTS: MetricConfig = MetricConfig::Points {
    for_win: 1.0,
    for_draw: 0.5,
    for_bye: 1.0,
};

/// The classical Sonneborn-Berger weights: the points of a beaten opponent count fully, those of
/// an opponent that was played to a draw count half.
const DEFAULT_SONNEBORN_BERGER: MetricConfig = MetricConfig::SonnebornBerger {
    weight_for_win: 1.0,
    weight_for_draw: 0.5,
};

/// What the engine needs in order to run a phase: the format that schedules its matches, the
/// ranker that sorts its entrants and the calculators of its metrics, in the order the metrics
/// are calculated in.
pub(crate) type PhasePlugins = (
    Box<dyn PhaseFormat>,
    Box<dyn Ranker>,
    Vec<Box<dyn MetricsCalculator>>,
);

/// How the matches of a phase are scheduled, with the parameters of the format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatConfig {
    RoundRobin,
    SingleElimination {
        /// Whether the two entrants knocked out in the semifinals play one more match, for third
        /// place.
        third_place_match: bool,
    },
    DoubleElimination {
        /// Whether the champion of the winner bracket also needs to lose twice, i.e. plays the
        /// grand final once more if it loses it.
        grand_final_reset: bool,
    },
    RandomRounds {
        rounds: NonZeroU32,
    },
    MonsterDyp {
        rounds: NonZeroU32,
    },
}

impl FormatConfig {
    /// Every format, each with the parameters it is offered with.
    pub const ALL: &'static [Self] = &[
        Self::RoundRobin,
        Self::SingleElimination {
            third_place_match: true,
        },
        Self::DoubleElimination {
            grand_final_reset: true,
        },
        Self::RandomRounds {
            rounds: DEFAULT_RANDOM_ROUNDS,
        },
        Self::MonsterDyp {
            rounds: DEFAULT_MONSTER_DYP_ROUNDS,
        },
    ];

    /// The name the format is shown by, without its parameters.
    pub fn label(self) -> &'static str {
        match self {
            Self::RoundRobin => "round robin",
            Self::SingleElimination { .. } => "single elimination",
            Self::DoubleElimination { .. } => "double elimination",
            Self::RandomRounds { .. } => "random rounds",
            Self::MonsterDyp { .. } => "Monster DYP",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::RoundRobin => "everyone plays everyone",
            Self::SingleElimination { .. } => "the loser of a match is out, draws are not allowed",
            Self::DoubleElimination { .. } => {
                "an entrant is out after its second loss, draws are not allowed"
            }
            Self::RandomRounds { .. } => "randomly chosen rounds of a round robin",
            Self::MonsterDyp { .. } => {
                "every round, entrants are drawn into teams of two that play each other"
            }
        }
    }

    /// The metrics calculated for a phase of this format unless an operator picks others.
    ///
    /// Only the metrics worth showing in the ranking are listed; every metric one of them is
    /// derived from is calculated as well.
    pub fn default_metrics(self) -> &'static [MetricKind] {
        match self {
            Self::RoundRobin => &[
                MetricKind::WinsDrawsLosses,
                MetricKind::Points,
                MetricKind::SonnebornBerger,
            ],
            Self::SingleElimination { .. } => {
                &[MetricKind::RoundOfFirstLoss, MetricKind::WinsDrawsLosses]
            }
            Self::DoubleElimination { .. } => {
                &[MetricKind::RoundOfElimination, MetricKind::WinsDrawsLosses]
            }
            Self::RandomRounds { .. } => &[
                MetricKind::WinsDrawsLosses,
                MetricKind::Points,
                MetricKind::Buchholz,
                MetricKind::SonnebornBerger,
                MetricKind::BuchholzOfBuchholz,
            ],
            Self::MonsterDyp { .. } => &[MetricKind::WinsDrawsLosses, MetricKind::Points],
        }
    }

    /// The metrics a phase of this format is ranked by unless an operator picks others, best
    /// first.
    pub fn default_ranked_by(self) -> &'static [Criterion] {
        match self {
            Self::RoundRobin => &[Criterion::MorePoints, Criterion::MoreSonnebornBerger],
            // the round of the first loss is the same for both entrants that lost the semifinals,
            // so the losses are what the match for third place is decided by in the ranking
            Self::SingleElimination {
                third_place_match: true,
            } => &[Criterion::LaterFirstLoss, Criterion::FewerLosses],
            Self::SingleElimination {
                third_place_match: false,
            } => &[Criterion::LaterFirstLoss],
            // an entrant drops into the loser bracket with its first loss, so it is the round it
            // was knocked out in that tells how far it got; until then, the losses tell those still
            // in the winner bracket from those that dropped
            Self::DoubleElimination { .. } => {
                &[Criterion::LaterElimination, Criterion::FewerLosses]
            }
            Self::RandomRounds { .. } => &[
                Criterion::MorePoints,
                Criterion::MoreBuchholz,
                Criterion::MoreSonnebornBerger,
                Criterion::MoreBuchholzOfBuchholz,
            ],
            // sitting a round out scores like a win, so the matches played need no tie break
            Self::MonsterDyp { .. } => &[Criterion::MorePoints],
        }
    }

    fn plugin(self) -> Box<dyn PhaseFormat> {
        match self {
            Self::RoundRobin => Box::new(RoundRobin),
            Self::SingleElimination {
                third_place_match: true,
            } => Box::new(SingleElimination::with_third_place_match()),
            Self::SingleElimination {
                third_place_match: false,
            } => Box::new(SingleElimination::new()),
            Self::DoubleElimination {
                grand_final_reset: true,
            } => Box::new(DoubleElimination::with_grand_final_reset()),
            Self::DoubleElimination {
                grand_final_reset: false,
            } => Box::new(DoubleElimination::new()),
            Self::RandomRounds { rounds } => Box::new(RandomRounds::new(rounds.get())),
            Self::MonsterDyp { rounds } => Box::new(MonsterDyp::new(rounds.get())),
        }
    }
}

/// A metric calculated for every entrant of a phase, with the parameters it is calculated with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MetricConfig {
    WinsDrawsLosses,
    /// Scores every win with `for_win`, every draw with `for_draw` and every bye with `for_bye`
    /// points.
    Points {
        for_win: f32,
        for_draw: f32,
        for_bye: f32,
    },
    Matches,
    RoundOfFirstLoss,
    RoundOfElimination,
    Buchholz,
    /// Counts the points of every beaten opponent with `weight_for_win` and those of every
    /// opponent that was played to a draw with `weight_for_draw`.
    SonnebornBerger {
        weight_for_win: f32,
        weight_for_draw: f32,
    },
    BuchholzOfBuchholz,
}

impl MetricConfig {
    /// Which metric this is, i.e. it without its parameters.
    pub fn kind(self) -> MetricKind {
        match self {
            Self::WinsDrawsLosses => MetricKind::WinsDrawsLosses,
            Self::Points { .. } => MetricKind::Points,
            Self::Matches => MetricKind::Matches,
            Self::RoundOfFirstLoss => MetricKind::RoundOfFirstLoss,
            Self::RoundOfElimination => MetricKind::RoundOfElimination,
            Self::Buchholz => MetricKind::Buchholz,
            Self::SonnebornBerger { .. } => MetricKind::SonnebornBerger,
            Self::BuchholzOfBuchholz => MetricKind::BuchholzOfBuchholz,
        }
    }

    fn calculator(self) -> Box<dyn MetricsCalculator> {
        match self {
            Self::WinsDrawsLosses => Box::new(WinsDrawsLossesCalculator),
            Self::Points {
                for_win,
                for_draw,
                for_bye,
            } => Box::new(PointsCalculator::new(for_win, for_draw).with_points_for_bye(for_bye)),
            Self::Matches => Box::new(MatchesCalculator),
            Self::RoundOfFirstLoss => Box::new(RoundOfFirstLossCalculator),
            Self::RoundOfElimination => Box::new(RoundOfEliminationCalculator),
            Self::Buchholz => Box::new(BuchholzCalculator),
            Self::SonnebornBerger {
                weight_for_win,
                weight_for_draw,
            } => Box::new(SonnebornBergerCalculator::new(
                weight_for_win,
                weight_for_draw,
            )),
            Self::BuchholzOfBuchholz => Box::new(BuchholzOfBuchholzCalculator),
        }
    }
}

/// A metric the CLI can calculate, without the parameters a [`MetricConfig`] gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    WinsDrawsLosses,
    Points,
    Matches,
    RoundOfFirstLoss,
    RoundOfElimination,
    Buchholz,
    SonnebornBerger,
    BuchholzOfBuchholz,
}

impl MetricKind {
    /// Every metric, in an order in which each one comes after the metrics it is derived from.
    ///
    /// This is the order a [`PhaseConfig`] keeps its metrics in, and therefore the order the engine
    /// calculates them in.
    pub const ALL: &'static [Self] = &[
        Self::WinsDrawsLosses,
        Self::Points,
        Self::Matches,
        Self::RoundOfFirstLoss,
        Self::RoundOfElimination,
        Self::Buchholz,
        Self::SonnebornBerger,
        Self::BuchholzOfBuchholz,
    ];

    /// The metrics an operator picks from: those that are shown in the ranking or can be ranked
    /// by.
    ///
    /// [`MetricKind::Matches`] is neither and is only ever added as a dependency of another metric.
    pub const SELECTABLE: &'static [Self] = &[
        Self::WinsDrawsLosses,
        Self::Points,
        Self::RoundOfFirstLoss,
        Self::RoundOfElimination,
        Self::Buchholz,
        Self::SonnebornBerger,
        Self::BuchholzOfBuchholz,
    ];

    /// The name the metric is offered and reported by, which the header of its column in the
    /// ranking may shorten.
    pub fn label(self) -> &'static str {
        match self {
            Self::WinsDrawsLosses => "W-D-L",
            Self::Points => "points",
            // not "matches played", which is what the criterion counting them is called, see
            // `Criterion::FewerMatchesPlayed`
            Self::Matches => "match list",
            Self::RoundOfFirstLoss => "round of the first loss",
            Self::RoundOfElimination => "round of elimination",
            Self::Buchholz => "Buchholz",
            Self::SonnebornBerger => "Sonneborn-Berger",
            Self::BuchholzOfBuchholz => "Buchholz of Buchholz",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::WinsDrawsLosses => "how many matches were won, drawn and lost",
            Self::Points => "scored for every win, every draw and every bye",
            Self::Matches => "the matches an entrant played",
            Self::RoundOfFirstLoss => "the later the better, never having lost is best",
            Self::RoundOfElimination => "the later the better, still being in is best",
            Self::Buchholz => "the points of all opponents",
            Self::SonnebornBerger => {
                "the points of the opponents that were beaten or played to a draw"
            }
            Self::BuchholzOfBuchholz => "the Buchholz of all opponents",
        }
    }

    /// The metrics this one is derived from, which have to be calculated before it.
    pub fn derived_from(self) -> &'static [Self] {
        match self {
            Self::WinsDrawsLosses
            | Self::Matches
            | Self::RoundOfFirstLoss
            | Self::RoundOfElimination => &[],
            Self::Points => &[Self::WinsDrawsLosses],
            Self::Buchholz | Self::SonnebornBerger => &[Self::Points, Self::Matches],
            Self::BuchholzOfBuchholz => &[Self::Buchholz, Self::Matches],
        }
    }

    /// The ways a ranking can be sorted by this metric, which is more than one where the metric
    /// holds several counts that each order a ranking on their own, and none where it has no order
    /// at all.
    pub fn criteria(self) -> &'static [Criterion] {
        match self {
            Self::WinsDrawsLosses => &[
                Criterion::MoreWins,
                Criterion::MoreDraws,
                Criterion::FewerLosses,
                Criterion::FewerMatchesPlayed,
            ],
            Self::Matches => &[],
            Self::Points => &[Criterion::MorePoints],
            Self::RoundOfFirstLoss => &[Criterion::LaterFirstLoss],
            Self::RoundOfElimination => &[Criterion::LaterElimination],
            Self::Buchholz => &[Criterion::MoreBuchholz],
            Self::SonnebornBerger => &[Criterion::MoreSonnebornBerger],
            Self::BuchholzOfBuchholz => &[Criterion::MoreBuchholzOfBuchholz],
        }
    }

    /// The metric with the parameters it is calculated with unless an operator changes them.
    pub fn with_defaults(self) -> MetricConfig {
        match self {
            Self::WinsDrawsLosses => MetricConfig::WinsDrawsLosses,
            Self::Points => DEFAULT_POINTS,
            Self::Matches => MetricConfig::Matches,
            Self::RoundOfFirstLoss => MetricConfig::RoundOfFirstLoss,
            Self::RoundOfElimination => MetricConfig::RoundOfElimination,
            Self::Buchholz => MetricConfig::Buchholz,
            Self::SonnebornBerger => DEFAULT_SONNEBORN_BERGER,
            Self::BuchholzOfBuchholz => MetricConfig::BuchholzOfBuchholz,
        }
    }
}

/// One way the engine can compare two entrants, i.e. a criterion a ranking can be sorted by.
///
/// This is not the same thing as a [`MetricKind`]: a metric with no order of its own is only ever
/// shown and never sorted by, and one that counts several things offers one criterion per count,
/// see [`MetricKind::criteria`]. A criterion whose metric has no single natural order says in its
/// name which way round it sorts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Criterion {
    MoreWins,
    MoreDraws,
    FewerLosses,
    /// Fewer of the matches that were won, drawn or lost, i.e. the same result out of fewer
    /// matches ranks higher. Used as a tie break.
    FewerMatchesPlayed,
    MorePoints,
    /// A later first loss, where never having lost is best.
    LaterFirstLoss,
    /// A later elimination, where still being in is best.
    LaterElimination,
    MoreBuchholz,
    MoreSonnebornBerger,
    MoreBuchholzOfBuchholz,
}

impl Criterion {
    /// The metric this criterion sorts by, which has to be calculated for the phase.
    pub fn kind(self) -> MetricKind {
        match self {
            Self::MoreWins | Self::MoreDraws | Self::FewerLosses | Self::FewerMatchesPlayed => {
                MetricKind::WinsDrawsLosses
            }
            Self::MorePoints => MetricKind::Points,
            Self::LaterFirstLoss => MetricKind::RoundOfFirstLoss,
            Self::LaterElimination => MetricKind::RoundOfElimination,
            Self::MoreBuchholz => MetricKind::Buchholz,
            Self::MoreSonnebornBerger => MetricKind::SonnebornBerger,
            Self::MoreBuchholzOfBuchholz => MetricKind::BuchholzOfBuchholz,
        }
    }

    /// The name the criterion is shown by, which is the label of its metric unless several criteria
    /// share that metric and need to be told apart, direction included.
    pub fn label(self) -> &'static str {
        match self {
            Self::MoreWins => "more wins",
            Self::MoreDraws => "more draws",
            Self::FewerLosses => "fewer losses",
            Self::FewerMatchesPlayed => "fewer matches played",
            _ => self.kind().label(),
        }
    }

    /// Appends this criterion to `metrics`, so that it breaks the ties of every criterion already
    /// in there.
    fn add_to(self, metrics: LexicographicalCriteria) -> LexicographicalCriteria {
        match self {
            Self::MoreWins => metrics.add_criterion(MoreWins),
            Self::MoreDraws => metrics.add_criterion(MoreDraws),
            Self::FewerLosses => metrics.add_criterion(FewerLosses),
            Self::FewerMatchesPlayed => metrics.add_criterion(FewerMatchesPlayed),
            Self::MorePoints => metrics.add_criterion(MorePoints),
            Self::LaterFirstLoss => metrics.add_criterion(LaterFirstLoss),
            Self::LaterElimination => metrics.add_criterion(LaterElimination),
            Self::MoreBuchholz => metrics.add_criterion(MoreBuchholz),
            Self::MoreSonnebornBerger => metrics.add_criterion(MoreSonnebornBerger),
            Self::MoreBuchholzOfBuchholz => metrics.add_criterion(MoreBuchholzOfBuchholz),
        }
    }
}

/// Everything the CLI can configure about the phase a competition is played in.
///
/// # Examples
/// ```
/// use tournament_cli::phase::{Criterion, FormatConfig, MetricKind, PhaseConfig};
///
/// // ranking by Sonneborn-Berger needs the points it sums, which need the results they are
/// // scored from, so all of them are calculated even though none was asked for
/// let config = PhaseConfig::new(
///     FormatConfig::RoundRobin,
///     [],
///     [Criterion::MoreSonnebornBerger],
/// );
///
/// assert_eq!(
///     config
///         .metrics()
///         .iter()
///         .map(|metric| metric.kind())
///         .collect::<Vec<_>>(),
///     [
///         MetricKind::WinsDrawsLosses,
///         MetricKind::Points,
///         MetricKind::Matches,
///         MetricKind::SonnebornBerger,
///     ]
/// );
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct PhaseConfig {
    format: FormatConfig,
    /// In the order the metrics are calculated in, see [`MetricKind::ALL`]. Holds every metric of
    /// `ranked_by` and every metric any of them is derived from.
    metrics: Vec<MetricConfig>,
    /// Best first, every metric breaking the ties of the ones before it. May be empty, which
    /// leaves every entrant tied for the first rank.
    ranked_by: Vec<Criterion>,
}

impl PhaseConfig {
    /// Configures a phase played in `format` that calculates `metrics` and is ranked by
    /// `ranked_by` lexicographically.
    ///
    /// Everything `ranked_by` sorts by and every metric a calculated metric is derived from is
    /// calculated as well, with default parameters if `metrics` does not give any. A metric listed
    /// twice is calculated once, with the parameters it is listed with first.
    pub fn new(
        format: FormatConfig,
        metrics: impl IntoIterator<Item = MetricConfig>,
        ranked_by: impl IntoIterator<Item = Criterion>,
    ) -> Self {
        // a metric cannot break its own ties, so one that is sorted by twice is only kept where
        // it is sorted by first
        let mut ranking: Vec<Criterion> = Vec::new();
        for metric in ranked_by {
            if !ranking.contains(&metric) {
                ranking.push(metric);
            }
        }
        let mut configured = Vec::new();
        for metric in metrics
            .into_iter()
            .chain(ranking.iter().map(|metric| metric.kind().with_defaults()))
        {
            if !configured
                .iter()
                .any(|other: &MetricConfig| other.kind() == metric.kind())
            {
                configured.push(metric);
            }
        }
        // every metric a calculated one is derived from is calculated too, and since a dependency
        // can itself be derived, the list grows while it is walked
        let mut needed: Vec<MetricKind> = configured.iter().map(|metric| metric.kind()).collect();
        let mut index = 0;
        while let Some(&kind) = needed.get(index) {
            for &dependency in kind.derived_from() {
                if !needed.contains(&dependency) {
                    needed.push(dependency);
                }
            }
            index += 1;
        }
        Self {
            format,
            metrics: MetricKind::ALL
                .iter()
                .filter(|kind| needed.contains(kind))
                .map(|&kind| {
                    configured
                        .iter()
                        .copied()
                        .find(|metric| metric.kind() == kind)
                        .unwrap_or_else(|| kind.with_defaults())
                })
                .collect(),
            ranked_by: ranking,
        }
    }

    /// The configuration `format` comes with, which is what an operator gets without changing
    /// anything.
    pub fn with_defaults(format: FormatConfig) -> Self {
        Self::new(
            format,
            format
                .default_metrics()
                .iter()
                .map(|kind| kind.with_defaults()),
            format.default_ranked_by().iter().copied(),
        )
    }

    /// The configuration a preset of `add-phase` stands for.
    pub fn preset(preset: PhasePreset) -> Self {
        Self::with_defaults(preset.format())
    }

    pub fn format(&self) -> FormatConfig {
        self.format
    }

    /// The metrics calculated for the phase, in the order they are calculated in.
    pub fn metrics(&self) -> &[MetricConfig] {
        &self.metrics
    }

    /// The metrics the phase is ranked by, best first.
    pub fn ranked_by(&self) -> &[Criterion] {
        &self.ranked_by
    }

    /// Names the format and the ranking in one line, the way a phase that was just added is
    /// reported.
    ///
    /// # Examples
    /// ```
    /// use tournament_cli::phase::{FormatConfig, PhaseConfig};
    ///
    /// assert_eq!(
    ///     PhaseConfig::with_defaults(FormatConfig::RoundRobin).summary(),
    ///     "round robin, ranked by points, then Sonneborn-Berger"
    /// );
    /// ```
    pub fn summary(&self) -> String {
        let format = match self.format {
            FormatConfig::RandomRounds { rounds } => format!("{rounds} random rounds"),
            FormatConfig::MonsterDyp { rounds } => format!("Monster DYP of {rounds} rounds"),
            FormatConfig::SingleElimination {
                third_place_match: true,
            } => "single elimination with a match for third place".to_owned(),
            FormatConfig::DoubleElimination {
                grand_final_reset: true,
            } => "double elimination with a grand final reset".to_owned(),
            format => format.label().to_owned(),
        };
        if self.ranked_by.is_empty() {
            return format!("{format}, not ranked by any metric");
        }
        let ranked_by: Vec<_> = self.ranked_by.iter().map(|metric| metric.label()).collect();
        format!("{format}, ranked by {}", ranked_by.join(", then "))
    }

    /// The plugins the engine runs the configured phase with, see [`PhasePlugins`].
    pub(crate) fn into_plugins(self) -> PhasePlugins {
        let metrics = self
            .ranked_by
            .iter()
            .fold(LexicographicalCriteria::new(), |metrics, ranking_metric| {
                ranking_metric.add_to(metrics)
            });
        (
            self.format.plugin(),
            Box::new(HierarchicalRanker::new(vec![Box::new(metrics)])),
            self.metrics
                .into_iter()
                .map(MetricConfig::calculator)
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(config: &PhaseConfig) -> Vec<MetricKind> {
        config
            .metrics()
            .iter()
            .map(|metric| metric.kind())
            .collect()
    }

    /// The engine calculates the metrics of a phase in the order it is given them, so a metric
    /// that is derived from another one only works if that one is calculated first. Every
    /// configuration orders its metrics by [`MetricKind::ALL`], which therefore has to be an
    /// order in which no metric comes before one it is derived from.
    #[test]
    fn every_metric_is_listed_after_the_ones_it_is_derived_from() {
        for (index, kind) in MetricKind::ALL.iter().enumerate() {
            for dependency in kind.derived_from() {
                let position = MetricKind::ALL
                    .iter()
                    .position(|other| other == dependency)
                    .expect("every metric a metric is derived from must be listed too");
                assert!(
                    position < index,
                    "{kind:?} is derived from {dependency:?}, which is calculated after it"
                );
            }
        }
    }

    /// Only a metric that is both offered and ordered can be ranked by, and every metric that is
    /// neither shown nor ranked by would only clutter the picker.
    #[test]
    fn every_metric_but_the_bookkeeping_one_is_offered() {
        for kind in MetricKind::ALL {
            let offered = MetricKind::SELECTABLE.contains(kind);
            assert_eq!(
                offered,
                *kind != MetricKind::Matches,
                "{kind:?} is offered: {offered}"
            );
        }
    }

    /// Every criterion is offered by the metric it sorts by, which is what makes
    /// [`PhaseConfig::new`] calculate that metric for a phase that is ranked by the criterion.
    #[test]
    fn every_criterion_a_metric_offers_sorts_by_that_metric() {
        for &kind in MetricKind::ALL {
            for &ranking_metric in kind.criteria() {
                assert_eq!(
                    ranking_metric.kind(),
                    kind,
                    "{kind:?} offers {ranking_metric:?}, which sorts by {:?}",
                    ranking_metric.kind()
                );
            }
        }
    }

    /// The ranking is picked criterion by criterion, so two criteria of the same metric have to be
    /// told apart by their labels.
    #[test]
    fn criteria_of_the_same_metric_have_distinct_labels() {
        for &kind in MetricKind::ALL {
            let labels: Vec<_> = kind
                .criteria()
                .iter()
                .map(|metric| metric.label())
                .collect();

            for (index, label) in labels.iter().enumerate() {
                assert!(
                    !labels[..index].contains(label),
                    "{kind:?} offers two criteria labelled {label}"
                );
            }
        }
    }

    /// Wins, draws and losses are three criteria of one metric, so ranking by several of them
    /// calculates that metric once.
    #[test]
    fn ranking_by_wins_and_losses_calculates_the_wins_draws_losses_metric_once() {
        let config = PhaseConfig::new(
            FormatConfig::RoundRobin,
            [],
            [Criterion::MoreWins, Criterion::FewerLosses],
        );

        assert_eq!(kinds(&config), [MetricKind::WinsDrawsLosses]);
        assert_eq!(
            config.summary(),
            "round robin, ranked by more wins, then fewer losses"
        );
    }

    /// A knockout that plays for third place needs a criterion that tells its winner from its
    /// loser, because the round of their first loss is the semifinal either way.
    #[test]
    fn a_knockout_that_plays_for_third_place_also_ranks_by_the_losses() {
        let config = PhaseConfig::with_defaults(FormatConfig::SingleElimination {
            third_place_match: true,
        });

        assert_eq!(
            config.ranked_by(),
            [Criterion::LaterFirstLoss, Criterion::FewerLosses]
        );
        assert_eq!(
            config.summary(),
            "single elimination with a match for third place, ranked by round of the first loss, \
             then fewer losses"
        );
    }

    #[test]
    fn a_knockout_that_ends_with_the_final_only_ranks_by_the_round_of_the_first_loss() {
        let config = PhaseConfig::with_defaults(FormatConfig::SingleElimination {
            third_place_match: false,
        });

        assert_eq!(config.ranked_by(), [Criterion::LaterFirstLoss]);
        assert_eq!(
            config.summary(),
            "single elimination, ranked by round of the first loss"
        );
    }

    #[test]
    fn a_double_elimination_ranks_by_the_round_of_elimination_then_the_losses() {
        let config = PhaseConfig::with_defaults(FormatConfig::DoubleElimination {
            grand_final_reset: true,
        });

        assert_eq!(
            config.ranked_by(),
            [Criterion::LaterElimination, Criterion::FewerLosses]
        );
        assert_eq!(
            kinds(&config),
            [MetricKind::WinsDrawsLosses, MetricKind::RoundOfElimination]
        );
        assert_eq!(
            config.summary(),
            "double elimination with a grand final reset, ranked by round of elimination, then \
             fewer losses"
        );
    }

    #[test]
    fn a_preset_is_the_configuration_of_its_format() {
        let rounds = NonZeroU32::new(7).unwrap();

        assert_eq!(
            PhaseConfig::preset(PhasePreset::RandomRounds { rounds }),
            PhaseConfig::with_defaults(FormatConfig::RandomRounds { rounds })
        );
    }

    #[test]
    fn a_metric_is_calculated_once_with_the_parameters_it_is_given_first() {
        let config = PhaseConfig::new(
            FormatConfig::RoundRobin,
            [
                MetricConfig::Points {
                    for_win: 2.0,
                    for_draw: 1.0,
                    for_bye: 2.0,
                },
                MetricConfig::Points {
                    for_win: 1.0,
                    for_draw: 0.5,
                    for_bye: 0.0,
                },
            ],
            [],
        );

        assert_eq!(
            config.metrics(),
            [
                MetricConfig::WinsDrawsLosses,
                MetricConfig::Points {
                    for_win: 2.0,
                    for_draw: 1.0,
                    for_bye: 2.0,
                },
            ]
        );
    }

    /// A metric picked with parameters keeps them even when something ranks by it, which adds it
    /// a second time.
    #[test]
    fn ranking_by_a_configured_metric_keeps_its_parameters() {
        let points = MetricConfig::Points {
            for_win: 1.0,
            for_draw: 0.5,
            for_bye: 1.0,
        };

        let config = PhaseConfig::new(FormatConfig::RoundRobin, [points], [Criterion::MorePoints]);

        assert!(config.metrics().contains(&points));
    }

    #[test]
    fn every_metric_a_calculated_one_is_derived_from_is_calculated_too() {
        let config = PhaseConfig::new(
            FormatConfig::RandomRounds {
                rounds: DEFAULT_RANDOM_ROUNDS,
            },
            [MetricConfig::BuchholzOfBuchholz],
            [],
        );

        assert_eq!(
            kinds(&config),
            [
                MetricKind::WinsDrawsLosses,
                MetricKind::Points,
                MetricKind::Matches,
                MetricKind::Buchholz,
                MetricKind::BuchholzOfBuchholz,
            ],
            "Buchholz of Buchholz needs the Buchholz it sums, which needs points and the matches \
             they were scored in"
        );
    }

    #[test]
    fn a_metric_that_is_only_ranked_by_is_calculated() {
        let config = PhaseConfig::new(
            FormatConfig::SingleElimination {
                third_place_match: false,
            },
            [],
            [Criterion::LaterFirstLoss],
        );

        assert_eq!(kinds(&config), [MetricKind::RoundOfFirstLoss]);
    }

    #[test]
    fn a_phase_can_be_configured_without_a_ranking() {
        let config = PhaseConfig::new(
            FormatConfig::RoundRobin,
            [MetricConfig::WinsDrawsLosses],
            [],
        );

        assert!(config.ranked_by().is_empty());
        assert_eq!(kinds(&config), [MetricKind::WinsDrawsLosses]);
        assert_eq!(
            config.summary(),
            "round robin, not ranked by any metric",
            "a ranking that sorts by nothing leaves every entrant tied"
        );
    }

    #[test]
    fn the_summary_names_the_rounds_of_the_random_rounds_format() {
        let config = PhaseConfig::with_defaults(FormatConfig::RandomRounds {
            rounds: NonZeroU32::new(3).unwrap(),
        });

        assert_eq!(
            config.summary(),
            "3 random rounds, ranked by points, then Buchholz, then Sonneborn-Berger, then \
             Buchholz of Buchholz"
        );
    }

    #[test]
    fn the_summary_names_the_rounds_of_the_monster_dyp() {
        let config = PhaseConfig::with_defaults(FormatConfig::MonsterDyp {
            rounds: NonZeroU32::new(4).unwrap(),
        });

        assert_eq!(
            config.summary(),
            "Monster DYP of 4 rounds, ranked by points"
        );
    }

    /// Every format is offered with parameters that are valid on their own, so that picking one
    /// and accepting everything else configures a phase that can be played.
    #[test]
    fn every_format_is_offered_with_a_ranking_it_calculates() {
        for &format in FormatConfig::ALL {
            let config = PhaseConfig::with_defaults(format);

            for ranked_by in config.ranked_by() {
                assert!(
                    kinds(&config).contains(&ranked_by.kind()),
                    "{format:?} ranks by {ranked_by:?} without calculating it"
                );
            }
        }
    }
}
