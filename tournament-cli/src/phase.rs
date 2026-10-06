//! The phase a competition is played in, as an operator configures it.
//!
//! A [`PhaseConfig`] holds everything the CLI can decide about that phase: the format its matches
//! are scheduled by, the metrics calculated for every entrant, and the order in which those
//! metrics rank them. A preset of `add-phase` is nothing but the configuration a format comes
//! with, and configuring a phase at the prompt (see [`crate::wizard`]) is changing that
//! configuration question by question, so both produce a [`PhaseConfig`] and
//! [`Session::add_phase`] turns it into the plugins of the engine.
//!
//! [`Session::add_phase`]: crate::session::Session::add_phase

use std::num::NonZeroU32;

use tournament_engine::plugins::{
    metrics_calculator::{
        MetricsCalculator,
        buchholz::{
            BuchholzCalculator, BuchholzGetter, BuchholzOfBuchholzCalculator,
            BuchholzOfBuchholzGetter,
        },
        matches::MatchesCalculator,
        points::{PointsCalculator, PointsGetter},
        round_of_first_loss::{RoundOfFirstLossCalculator, RoundOfFirstLossGetter},
        sonneborn_berger::{SonnebornBergerCalculator, SonnebornBergerGetter},
        wins_draws_losses::WinsDrawsLossesCalculator,
    },
    phase_format::{
        PhaseFormat, random_rounds::RandomRounds, round_robin::RoundRobin,
        single_elimination::SingleElimination,
    },
    ranker::{HierarchicalRanker, LexicographicalMetrics, Ranker},
};

use crate::command::PhasePreset;

/// The number of rounds the random rounds format is offered with.
const DEFAULT_RANDOM_ROUNDS: NonZeroU32 = NonZeroU32::new(5).unwrap();

/// The scoring system of most association football leagues.
const DEFAULT_POINTS: MetricConfig = MetricConfig::Points {
    for_win: 1.0,
    for_draw: 0.5,
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
    SingleElimination,
    RandomRounds { rounds: NonZeroU32 },
}

impl FormatConfig {
    /// Every format, each with the parameters it is offered with.
    pub const ALL: &'static [Self] = &[
        Self::RoundRobin,
        Self::SingleElimination,
        Self::RandomRounds {
            rounds: DEFAULT_RANDOM_ROUNDS,
        },
    ];

    /// The name the format is shown by, without its parameters.
    pub fn label(self) -> &'static str {
        match self {
            Self::RoundRobin => "round robin",
            Self::SingleElimination => "single elimination",
            Self::RandomRounds { .. } => "random rounds",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::RoundRobin => "everyone plays everyone",
            Self::SingleElimination => "the loser of a match is out, draws are not allowed",
            Self::RandomRounds { .. } => "randomly chosen rounds of a round robin",
        }
    }

    /// The metrics calculated for a phase of this format unless an operator picks others.
    ///
    /// Every metric these are derived from is calculated too, even when it is not listed here, so
    /// the list only holds the metrics that are worth showing in the ranking.
    pub fn default_metrics(self) -> &'static [MetricKind] {
        match self {
            Self::RoundRobin => &[
                MetricKind::WinsDrawsLosses,
                MetricKind::Points,
                MetricKind::SonnebornBerger,
            ],
            Self::SingleElimination => &[MetricKind::RoundOfFirstLoss, MetricKind::WinsDrawsLosses],
            Self::RandomRounds { .. } => &[
                MetricKind::WinsDrawsLosses,
                MetricKind::Points,
                MetricKind::Buchholz,
                MetricKind::SonnebornBerger,
                MetricKind::BuchholzOfBuchholz,
            ],
        }
    }

    /// The metrics a phase of this format is ranked by unless an operator picks others, best
    /// first.
    pub fn default_ranked_by(self) -> &'static [RankingMetric] {
        match self {
            Self::RoundRobin => &[RankingMetric::Points, RankingMetric::SonnebornBerger],
            Self::SingleElimination => &[RankingMetric::RoundOfFirstLoss],
            Self::RandomRounds { .. } => &[
                RankingMetric::Points,
                RankingMetric::Buchholz,
                RankingMetric::SonnebornBerger,
                RankingMetric::BuchholzOfBuchholz,
            ],
        }
    }

    fn plugin(self) -> Box<dyn PhaseFormat> {
        match self {
            Self::RoundRobin => Box::new(RoundRobin),
            Self::SingleElimination => Box::new(SingleElimination),
            Self::RandomRounds { rounds } => Box::new(RandomRounds::new(rounds.get())),
        }
    }
}

/// A metric calculated for every entrant of a phase, with the parameters it is calculated with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MetricConfig {
    WinsDrawsLosses,
    /// Scores every win with `for_win` and every draw with `for_draw` points.
    Points {
        for_win: f32,
        for_draw: f32,
    },
    Matches,
    RoundOfFirstLoss,
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
            Self::Buchholz => MetricKind::Buchholz,
            Self::SonnebornBerger { .. } => MetricKind::SonnebornBerger,
            Self::BuchholzOfBuchholz => MetricKind::BuchholzOfBuchholz,
        }
    }

    fn calculator(self) -> Box<dyn MetricsCalculator> {
        match self {
            Self::WinsDrawsLosses => Box::new(WinsDrawsLossesCalculator),
            Self::Points { for_win, for_draw } => {
                Box::new(PointsCalculator::new(for_win, for_draw))
            }
            Self::Matches => Box::new(MatchesCalculator),
            Self::RoundOfFirstLoss => Box::new(RoundOfFirstLossCalculator),
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
    Buchholz,
    SonnebornBerger,
    BuchholzOfBuchholz,
}

impl MetricKind {
    /// Every metric, in an order in which each one comes after the metrics it is derived from.
    ///
    /// The engine calculates the metrics of a phase in the order they are given to it, and a
    /// derived metric only sees the updated values it is derived from if those were calculated
    /// first, which is why this is the order a [`PhaseConfig`] keeps its metrics in.
    pub const ALL: &'static [Self] = &[
        Self::WinsDrawsLosses,
        Self::Points,
        Self::Matches,
        Self::RoundOfFirstLoss,
        Self::Buchholz,
        Self::SonnebornBerger,
        Self::BuchholzOfBuchholz,
    ];

    /// The metrics an operator picks from: those that are shown in the ranking or can be ranked
    /// by.
    ///
    /// [`MetricKind::Matches`] is neither — it is the bookkeeping the metrics derived from it
    /// need — so it is never picked and only ever added as a dependency.
    pub const SELECTABLE: &'static [Self] = &[
        Self::WinsDrawsLosses,
        Self::Points,
        Self::RoundOfFirstLoss,
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
            Self::Matches => "matches played",
            Self::RoundOfFirstLoss => "round of the first loss",
            Self::Buchholz => "Buchholz",
            Self::SonnebornBerger => "Sonneborn-Berger",
            Self::BuchholzOfBuchholz => "Buchholz of Buchholz",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::WinsDrawsLosses => "how many matches were won, drawn and lost",
            Self::Points => "scored for every win and every draw",
            Self::Matches => "the matches an entrant played",
            Self::RoundOfFirstLoss => "the later the better, never having lost is best",
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
            Self::WinsDrawsLosses | Self::Matches | Self::RoundOfFirstLoss => &[],
            Self::Points => &[Self::WinsDrawsLosses],
            Self::Buchholz | Self::SonnebornBerger => &[Self::Points, Self::Matches],
            Self::BuchholzOfBuchholz => &[Self::Buchholz, Self::Matches],
        }
    }

    /// How a ranking is ranked by this metric, if it can be ranked by it at all.
    pub fn as_ranking_metric(self) -> Option<RankingMetric> {
        match self {
            Self::WinsDrawsLosses | Self::Matches => None,
            Self::Points => Some(RankingMetric::Points),
            Self::RoundOfFirstLoss => Some(RankingMetric::RoundOfFirstLoss),
            Self::Buchholz => Some(RankingMetric::Buchholz),
            Self::SonnebornBerger => Some(RankingMetric::SonnebornBerger),
            Self::BuchholzOfBuchholz => Some(RankingMetric::BuchholzOfBuchholz),
        }
    }

    /// The metric with the parameters it is calculated with unless an operator changes them.
    pub fn with_defaults(self) -> MetricConfig {
        match self {
            Self::WinsDrawsLosses => MetricConfig::WinsDrawsLosses,
            Self::Points => DEFAULT_POINTS,
            Self::Matches => MetricConfig::Matches,
            Self::RoundOfFirstLoss => MetricConfig::RoundOfFirstLoss,
            Self::Buchholz => MetricConfig::Buchholz,
            Self::SonnebornBerger => DEFAULT_SONNEBORN_BERGER,
            Self::BuchholzOfBuchholz => MetricConfig::BuchholzOfBuchholz,
        }
    }
}

/// A metric a ranking can be sorted by, i.e. one the engine compares two entrants by.
///
/// Every other [`MetricKind`] is only ever shown, which is what keeps a ranking from being asked
/// to sort by a metric that has no order, see [`MetricKind::as_ranking_metric`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RankingMetric {
    Points,
    RoundOfFirstLoss,
    Buchholz,
    SonnebornBerger,
    BuchholzOfBuchholz,
}

impl RankingMetric {
    pub fn kind(self) -> MetricKind {
        match self {
            Self::Points => MetricKind::Points,
            Self::RoundOfFirstLoss => MetricKind::RoundOfFirstLoss,
            Self::Buchholz => MetricKind::Buchholz,
            Self::SonnebornBerger => MetricKind::SonnebornBerger,
            Self::BuchholzOfBuchholz => MetricKind::BuchholzOfBuchholz,
        }
    }

    /// The name the metric is shown by, see [`MetricKind::label`].
    pub fn label(self) -> &'static str {
        self.kind().label()
    }

    /// Appends this metric to `metrics`, so that it breaks the ties of every metric already in
    /// there.
    ///
    /// Adding the getter of the metric here rather than handing one out keeps the metric a
    /// ranking is sorted by and the way it is read out of that ranking in one place.
    fn add_to(self, metrics: LexicographicalMetrics) -> LexicographicalMetrics {
        match self {
            Self::Points => metrics.add_metric(PointsGetter),
            Self::RoundOfFirstLoss => metrics.add_metric(RoundOfFirstLossGetter),
            Self::Buchholz => metrics.add_metric(BuchholzGetter),
            Self::SonnebornBerger => metrics.add_metric(SonnebornBergerGetter),
            Self::BuchholzOfBuchholz => metrics.add_metric(BuchholzOfBuchholzGetter),
        }
    }
}

/// Everything the CLI can configure about the phase a competition is played in.
///
/// # Examples
/// ```
/// use tournament_cli::phase::{FormatConfig, MetricKind, PhaseConfig, RankingMetric};
///
/// // ranking by Sonneborn-Berger needs the points it sums, which need the results they are
/// // scored from, so all of them are calculated even though none was asked for
/// let config = PhaseConfig::new(
///     FormatConfig::RoundRobin,
///     [],
///     [RankingMetric::SonnebornBerger],
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
    ranked_by: Vec<RankingMetric>,
}

impl PhaseConfig {
    /// Configures a phase played in `format` that calculates `metrics` and is ranked by
    /// `ranked_by` lexicographically.
    ///
    /// Everything `ranked_by` sorts by and every metric a calculated metric is derived from is
    /// calculated as well, with default parameters if `metrics` does not give any, so that no
    /// configuration can ask for a metric that cannot be calculated. A metric listed twice is
    /// calculated once, with the parameters it is listed with first.
    pub fn new(
        format: FormatConfig,
        metrics: impl IntoIterator<Item = MetricConfig>,
        ranked_by: impl IntoIterator<Item = RankingMetric>,
    ) -> Self {
        // a metric cannot break its own ties, so one that is sorted by twice is only kept where
        // it is sorted by first
        let mut ranking: Vec<RankingMetric> = Vec::new();
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
    pub fn ranked_by(&self) -> &[RankingMetric] {
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
            .fold(LexicographicalMetrics::new(), |metrics, ranking_metric| {
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
                },
                MetricConfig::Points {
                    for_win: 1.0,
                    for_draw: 0.5,
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
                    for_draw: 1.0
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
        };

        let config = PhaseConfig::new(FormatConfig::RoundRobin, [points], [RankingMetric::Points]);

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
            FormatConfig::SingleElimination,
            [],
            [RankingMetric::RoundOfFirstLoss],
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
