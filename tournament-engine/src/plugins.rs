//! The plugin axes of a phase and the plugins of this crate, built on `plugin_factory`.
//!
//! A phase is played in one [`PhaseFormat`], ranked by a list of [`SliceRanker`]s and calculates
//! the metrics of a list of [`MetricsCalculator`]s; [`PhasePlugins`] is that composite. Every
//! plugin registers itself in the catalog of its axis, see [`plugin_factory::catalog`], so a
//! consumer configuring a phase finds them there, and declares what it provides to and requires of
//! its phase, see [`facts`] and [`plugin_factory::compatibility`].

use std::mem;

use plugin_factory::{
    compatibility::{self, Incompatibility},
    draft::{self, Draft, Issue},
    schema::Configurable,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    entities::competition::Competition,
    plugins::{
        metrics_calculator::MetricsCalculator, phase_format::PhaseFormat, ranker::SliceRanker,
    },
};

pub mod facts;
pub mod metrics_calculator;
pub mod phase_format;
pub mod ranker;

/// The plugins of one phase: the format its matches are scheduled by, the slice rankers it is
/// ranked by and the calculators of its metrics.
///
/// It is configured as a composite of `plugin_factory`, e.g. through a [`Draft`] of it, whose
/// [`draft::finish`] takes [`Competition::declare_phase_context`] as the context.
#[derive(Debug, Clone, Serialize, Deserialize, Configurable)]
pub struct PhasePlugins {
    #[plugin(label = "Format", description = "how the matches are scheduled")]
    pub(crate) format: Box<dyn PhaseFormat>,
    #[serde(default)]
    #[plugin(
        label = "Ranked by",
        description = "best first, every criterion only breaks the ties of the ones before it"
    )]
    pub(crate) ranking: Vec<Box<dyn SliceRanker>>,
    #[serde(default)]
    #[plugin(label = "Metrics", description = "calculated for every entrant")]
    pub(crate) metrics: Vec<Box<dyn MetricsCalculator>>,
}

impl PhasePlugins {
    /// The plugins of a phase played in `format`, ranked by `ranking`, best first, and calculating
    /// the metrics of `metrics`.
    pub fn new(
        format: Box<dyn PhaseFormat>,
        ranking: Vec<Box<dyn SliceRanker>>,
        metrics: Vec<Box<dyn MetricsCalculator>>,
    ) -> Self {
        Self {
            format,
            ranking,
            metrics,
        }
    }

    pub fn format(&self) -> &dyn PhaseFormat {
        &*self.format
    }

    /// The slice rankers, best first.
    pub fn ranking(&self) -> &[Box<dyn SliceRanker>] {
        &self.ranking
    }

    /// The metrics calculators, in the order they run in once
    /// [`AddPhase::new`](crate::transitions::add_phase::AddPhase::new) ordered them.
    pub fn metrics(&self) -> &[Box<dyn MetricsCalculator>] {
        &self.metrics
    }

    /// These plugins with every fact settled that the catalog settles in a phase of
    /// `competition`, see [`draft::finish`]: e.g. the calculator of a metric another plugin
    /// requires, if it is the only one and needs no configuration.
    ///
    /// # Errors
    /// Returns every issue of the plugins so settled, e.g. a fact still nothing provides.
    ///
    /// # Panics
    /// Panics if a plugin serializes to something other than an answer to its schema, which the
    /// derive of a plugin rules out.
    pub fn settle(&self, competition: &Competition) -> Result<Self, Vec<Issue>> {
        let draft = Draft::from_built(self).expect("a built plugin is a configuration of itself");
        draft::finish(
            |declaration| competition.declare_phase_context(declaration),
            &draft,
        )
    }

    /// Orders the metrics calculators so that each runs after the calculators providing the facts
    /// it requires, keeping their order otherwise.
    ///
    /// # Errors
    /// Returns an error if calculators require facts of each other in a cycle.
    pub(crate) fn order_metrics(&mut self) -> Result<(), Incompatibility> {
        let order = compatibility::order(&self.metrics_as_plugins())?;
        let mut metrics: Vec<_> = mem::take(&mut self.metrics).into_iter().map(Some).collect();
        self.metrics = order
            .into_iter()
            .map(|index| {
                metrics[index]
                    .take()
                    .expect("an order holds every index once")
            })
            .collect();
        Ok(())
    }

    /// Checks that the metrics calculators are ordered as [`Self::order_metrics`] orders them.
    ///
    /// # Errors
    /// Returns an error if they are not, or if they require facts of each other in a cycle.
    pub(crate) fn check_order(&self) -> Result<(), OrderError> {
        let order = compatibility::order(&self.metrics_as_plugins())?;
        match order
            .into_iter()
            .enumerate()
            .find(|(position, index)| position != index)
        {
            Some((position, _)) => Err(OrderError::OutOfOrder {
                plugin: format!("{:?}", self.metrics[position]),
            }),
            None => Ok(()),
        }
    }

    fn metrics_as_plugins(&self) -> Vec<&dyn plugin_factory::Plugin> {
        self.metrics
            .iter()
            .map(|metric| &**metric as &dyn plugin_factory::Plugin)
            .collect()
    }
}

/// Why the metrics calculators of a phase are not in the order they run in.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum OrderError {
    #[error(transparent)]
    Cyclic(#[from] Incompatibility),
    #[error("{plugin} runs before a plugin providing a fact it requires")]
    OutOfOrder { plugin: String },
}

/// What the plugins of this crate declare, i.e. the facts they provide to and require of their
/// phase, and how a phase settles and orders them. The mechanism itself is tested in
/// `plugin_factory`.
#[cfg(test)]
mod tests {
    use std::{
        any::{TypeId, type_name},
        fmt::Debug,
        num::NonZeroU32,
    };

    use plugin_factory::{
        Plugin, PluginType, catalog::AnyAxis, compatibility::Declaration, config::PluginConfig,
        draft::Problem, register,
    };

    use super::*;
    use crate::{
        entities::competition::{Bye, Match, MatchResult, PhaseData, RankingId},
        map::Map,
        plugins::{
            facts::{EntrantsPerRanking, MatchAnnotation},
            metrics_calculator::{
                Metric,
                buchholz::{
                    Buchholz, BuchholzCalculator, BuchholzOfBuchholz, BuchholzOfBuchholzCalculator,
                },
                matches::{Matches, MatchesCalculator},
                points::{MorePoints, Points, PointsCalculator},
                round_of_elimination::{
                    Eliminates, LaterElimination, RoundOfEliminationCalculator,
                },
                round_of_first_loss::{LaterFirstLoss, RoundOfFirstLoss},
                sonneborn_berger::{
                    MoreSonnebornBerger, SonnebornBerger, SonnebornBergerCalculator,
                },
                wins_draws_losses::{FewerLosses, WinsDrawsLosses, WinsDrawsLossesCalculator},
            },
            phase_format::{
                double_elimination::{DoubleElimination, DoubleEliminationRound},
                monster_dyp::MonsterDyp,
                round_robin::RoundRobin,
                single_elimination::{KnockoutRound, SingleElimination},
            },
        },
        type_map::TypeMapValue,
    };

    /// The scoring system of chess: a win is worth a point, a draw half of one.
    const ONE_HALF: PointsCalculator = PointsCalculator::new(1.0, 0.5);
    /// The classic Sonneborn-Berger weights: a beaten opponent counts fully, an opponent played to
    /// a draw counts half.
    const FULL_HALF: SonnebornBergerCalculator = SonnebornBergerCalculator::new(1.0, 0.5);
    /// Two entrants per ranking, i.e. the rankings a draw pairing the entrants up leaves behind.
    const PAIRS: EntrantsPerRanking = EntrantsPerRanking(NonZeroU32::new(2).unwrap());

    /// The plugins of a phase played in `format`, ranked by `ranking` and calculating the metrics
    /// of `metrics`.
    fn phase(
        format: impl PhaseFormat + 'static,
        ranking: Vec<Box<dyn SliceRanker>>,
        metrics: Vec<Box<dyn MetricsCalculator>>,
    ) -> PhasePlugins {
        PhasePlugins::new(Box::new(format), ranking, metrics)
    }

    /// Whether `plugins` fit together in a competition, i.e. what `AddPhase` checks.
    fn check(plugins: &PhasePlugins) -> Result<(), Incompatibility> {
        check_in(
            |declaration| Competition::default().declare_phase_context(declaration),
            plugins,
        )
    }

    /// Like [`check`], for a phase whose context is `context` rather than a competition.
    fn check_in(
        context: impl FnOnce(&mut Declaration),
        plugins: &PhasePlugins,
    ) -> Result<(), Incompatibility> {
        compatibility::check(context, &plugins.plugins())
    }

    /// Whether `plugin` provides a fact of type `T`: it does exactly if it conflicts with a
    /// context providing one.
    fn provides<T: Debug + Default + Sync + 'static>(plugin: &dyn Plugin) -> bool {
        matches!(
            compatibility::check(
                |declaration| {
                    declaration.provide(T::default());
                },
                &[plugin]
            ),
            Err(Incompatibility::FactProvidedByContext { .. })
        )
    }

    /// The [`Debug`] forms of the metrics the calculators of `plugins` calculate, in the order
    /// they run in.
    fn metrics(plugins: &PhasePlugins) -> Vec<String> {
        plugins
            .metrics()
            .iter()
            .map(|calculator| format!("{:?}", calculator.default_metric_value()))
            .collect()
    }

    /// The [`Debug`] form of the default of each metric `M`.
    macro_rules! metrics {
        ($($metric:ty),* $(,)?) => {
            vec![$(format!("{:?}", <$metric>::default())),*]
        };
    }

    /// `plugins` settled in a competition and ordered, as a consumer adding them would.
    fn settled(plugins: &PhasePlugins) -> Result<PhasePlugins, Vec<Issue>> {
        let mut settled = plugins.settle(&Competition::default())?;
        settled.order_metrics().unwrap();
        Ok(settled)
    }

    #[test]
    fn a_calculator_runs_after_the_calculators_of_the_metrics_it_reads() {
        // listed so that every calculator reading a metric comes before the one calculating it
        let mut plugins = phase(
            RoundRobin,
            Vec::new(),
            vec![
                Box::new(BuchholzOfBuchholzCalculator),
                Box::new(FULL_HALF),
                Box::new(BuchholzCalculator),
                Box::new(ONE_HALF),
                Box::new(MatchesCalculator),
                Box::new(WinsDrawsLossesCalculator),
            ],
        );

        plugins.order_metrics().unwrap();

        assert_eq!(
            metrics(&plugins),
            metrics![
                Matches,
                WinsDrawsLosses,
                Points,
                // the Sonneborn-Berger keeps its place before the Buchholz, as neither reads the
                // other
                SonnebornBerger,
                Buchholz,
                BuchholzOfBuchholz,
            ]
        );
        assert_eq!(plugins.check_order(), Ok(()));
    }

    #[test]
    fn calculators_out_of_order_fail_the_order_check() {
        let plugins = phase(
            RoundRobin,
            Vec::new(),
            vec![Box::new(ONE_HALF), Box::new(WinsDrawsLossesCalculator)],
        );

        assert_eq!(
            plugins.check_order(),
            Err(OrderError::OutOfOrder {
                plugin: format!("{ONE_HALF:?}"),
            })
        );
    }

    /// Nobody configuring a phase has to know that the Sonneborn-Berger sums the points of the
    /// opponents a ranking met, nor that the points are scored from its results: every metric a
    /// configured calculator reads is calculated by the one calculator registered for it, as long
    /// as it needs no configuration.
    #[test]
    fn the_metrics_a_configured_calculator_reads_are_added_from_the_catalog() {
        let plugins = phase(
            RoundRobin,
            Vec::new(),
            vec![Box::new(FULL_HALF), Box::new(ONE_HALF)],
        );

        let settled = settled(&plugins).unwrap();

        assert_eq!(
            metrics(&settled),
            metrics![Matches, WinsDrawsLosses, Points, SonnebornBerger]
        );
        assert_eq!(check(&settled), Ok(()));
    }

    /// A metric is required by a criterion as well as by a calculator, so ranking by one that is
    /// not configured calculates it.
    #[test]
    fn a_metric_the_phase_is_ranked_by_is_added_from_the_catalog() {
        let plugins = phase(
            SingleElimination::new(),
            vec![Box::new(LaterFirstLoss)],
            Vec::new(),
        );

        let settled = settled(&plugins).unwrap();

        assert_eq!(metrics(&settled), metrics![RoundOfFirstLoss]);
        assert_eq!(check(&settled), Ok(()));
    }

    /// The points can be scored in many ways, so ranking by them without configuring how is an
    /// issue rather than a guess.
    #[test]
    fn a_metric_whose_calculator_has_to_be_configured_is_not_added() {
        let plugins = phase(RoundRobin, vec![Box::new(MorePoints)], Vec::new());

        let issues = plugins.settle(&Competition::default()).unwrap_err();

        assert!(
            issues.iter().any(|issue| matches!(
                issue.problem(),
                Problem::Unprovided { fact, providers, .. }
                    if *fact == type_name::<Points>()
                        && providers.contains(&PointsCalculator::ID)
            )),
            "got {issues:?}"
        );
    }

    /// A calculator is found and checked by the fact it declares, so a registered calculator that
    /// declares a metric other than the one it calculates would be added for the wrong metric and
    /// never for its own.
    #[test]
    fn every_registered_calculator_provides_the_metric_it_calculates() {
        for entry in AnyAxis::of::<dyn MetricsCalculator>().entries() {
            // every calculator of this crate offers an answer to each of its fields
            let mut config = PluginConfig::new(entry.id());
            for field in entry.schema().fields() {
                let default = field.default_value().expect("every field offers an answer");
                config = config.with(field.name(), default.clone());
            }
            let calculator = config.build::<dyn MetricsCalculator>().unwrap();
            let both: [&dyn Plugin; 2] = [&*calculator, &*calculator];

            // a calculator providing its metric twice conflicts with itself about that metric
            assert_eq!(
                compatibility::check(|_| {}, &both).err(),
                Some(Incompatibility::FactProvidedTwice {
                    fact: format!("{:?}", calculator.default_metric_value()),
                    first: format!("{calculator:?}"),
                    second: format!("{calculator:?}"),
                }),
                "{} does not provide the metric it calculates",
                entry.id()
            );
        }
    }

    #[test]
    fn a_round_robin_ranked_by_the_sonneborn_berger_fits_with_every_metric_it_reads() {
        let plugins = phase(
            RoundRobin,
            vec![Box::new(MorePoints), Box::new(MoreSonnebornBerger)],
            vec![
                Box::new(WinsDrawsLossesCalculator),
                Box::new(ONE_HALF),
                Box::new(MatchesCalculator),
                Box::new(FULL_HALF),
            ],
        );

        assert_eq!(check(&plugins), Ok(()));
    }

    #[test]
    fn a_metric_a_calculator_reads_has_to_be_calculated_as_well() {
        // the Sonneborn-Berger sums the points of the opponents a ranking met, which it reads from
        // the matches of that ranking
        let plugins = phase(
            RoundRobin,
            Vec::new(),
            vec![
                Box::new(WinsDrawsLossesCalculator),
                Box::new(ONE_HALF),
                Box::new(FULL_HALF),
            ],
        );

        assert_eq!(
            check(&plugins),
            Err(Incompatibility::FactNotProvided {
                fact: "the matches",
                required_by: format!("{FULL_HALF:?}"),
            })
        );
    }

    #[test]
    fn ranking_by_a_metric_nothing_calculates_is_an_error() {
        let plugins = phase(
            RoundRobin,
            vec![Box::new(MorePoints)],
            vec![Box::new(WinsDrawsLossesCalculator)],
        );

        assert_eq!(
            check(&plugins),
            Err(Incompatibility::FactNotProvided {
                fact: type_name::<Points>(),
                required_by: format!("{MorePoints:?}"),
            })
        );
    }

    /// A criterion requires its metric whether it compares it in the metric's own order or in one
    /// of its own.
    #[test]
    fn a_criterion_bringing_its_own_order_requires_its_metric_too() {
        let plugins = phase(RoundRobin, vec![Box::new(FewerLosses)], Vec::new());

        assert_eq!(
            check(&plugins),
            Err(Incompatibility::FactNotProvided {
                fact: type_name::<WinsDrawsLosses>(),
                required_by: format!("{FewerLosses:?}"),
            })
        );
    }

    #[test]
    fn the_round_of_elimination_needs_a_format_that_knocks_entrants_out() {
        let ranking: Vec<Box<dyn SliceRanker>> = vec![Box::new(LaterElimination)];
        let metrics: Vec<Box<dyn MetricsCalculator>> = vec![Box::new(RoundOfEliminationCalculator)];

        assert_eq!(
            check(&phase(
                DoubleElimination::new(),
                ranking.clone(),
                metrics.clone()
            )),
            Ok(())
        );
        assert_eq!(
            check(&phase(RoundRobin, ranking, metrics)),
            Err(Incompatibility::FactNotProvided {
                fact: "an elimination annotation on the matches",
                required_by: format!("{RoundOfEliminationCalculator:?}"),
            })
        );
    }

    /// The annotation naming the stage of a match is declared like every other one, so that a
    /// plugin reading it — a display of the bracket, say — can require it.
    #[test]
    fn a_bracket_format_provides_the_annotations_it_writes() {
        assert!(provides::<MatchAnnotation<KnockoutRound>>(
            &SingleElimination::new()
        ));
        assert!(provides::<MatchAnnotation<DoubleEliminationRound>>(
            &DoubleElimination::new()
        ));
        assert!(provides::<MatchAnnotation<Eliminates>>(
            &DoubleElimination::new()
        ));
        assert!(!provides::<MatchAnnotation<Eliminates>>(
            &SingleElimination::new()
        ));
    }

    #[test]
    fn a_monster_dyp_fits_a_competition_whose_rankings_are_single_entrants() {
        assert_eq!(
            check(&phase(MonsterDyp::new(3), Vec::new(), Vec::new())),
            Ok(())
        );
    }

    #[test]
    fn a_monster_dyp_cannot_follow_a_draw_that_paired_the_entrants_up() {
        let format = MonsterDyp::new(3);
        let pairs = |declaration: &mut Declaration| {
            declaration.provide(PAIRS);
        };

        assert_eq!(
            check_in(pairs, &phase(format.clone(), Vec::new(), Vec::new())),
            Err(Incompatibility::ConditionNotMet {
                condition: "rankings of single players",
                required_by: format!("{format:?}"),
                provided: format!("{PAIRS:?}"),
            })
        );
    }

    /// The metric of [`CircularCalculator`], which is never calculated.
    #[derive(Debug, Default, Clone, Serialize, Deserialize)]
    struct Circular;

    #[typetag::serde(name = "tournament_engine::plugins::tests::circular")]
    impl TypeMapValue for Circular {}

    /// The metric of [`Cycling`], which is never calculated.
    #[derive(Debug, Default, Clone, Serialize, Deserialize)]
    struct Cycled;

    #[typetag::serde(name = "tournament_engine::plugins::tests::cycled")]
    impl TypeMapValue for Cycled {}

    /// A calculator requiring the metric of [`Cycling`], which requires its own metric.
    #[derive(Debug, Clone, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "tournament_engine::plugins::tests::circular",
        label = "circular",
        declare_static = Self::declare_static
    )]
    struct CircularCalculator;

    impl CircularCalculator {
        fn declare_static(declaration: &mut Declaration) {
            declaration
                .provide(Circular)
                .require::<Cycled>("the cycled fact");
        }
    }

    /// A calculator requiring the metric of [`CircularCalculator`], which requires its own one.
    #[derive(Debug, Clone, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "tournament_engine::plugins::tests::cycling",
        label = "cycling",
        declare_static = Self::declare_static
    )]
    struct Cycling;

    impl Cycling {
        fn declare_static(declaration: &mut Declaration) {
            declaration
                .provide(Cycled)
                .require::<Circular>("the circular metric");
        }
    }

    /// Calculates nothing, for the calculators of this module that only exist for what they
    /// declare.
    macro_rules! calculates_nothing {
        ($calculator:ty, $metric:expr) => {
            #[register]
            impl MetricsCalculator for $calculator {
                fn default_metric_value(&self) -> Metric {
                    Box::new($metric)
                }

                fn update_after_match_result_changed(
                    &mut self,
                    _: &PhaseData,
                    _: &Match,
                    _: MatchResult,
                    _: &Map<TypeId, Vec<(RankingId, Metric)>>,
                ) -> Vec<(RankingId, Metric)> {
                    Vec::new()
                }

                fn update_after_bye(
                    &mut self,
                    _: &PhaseData,
                    _: &Bye,
                    _: &Map<TypeId, Vec<(RankingId, Metric)>>,
                ) -> Vec<(RankingId, Metric)> {
                    Vec::new()
                }
            }
        };
    }

    calculates_nothing!(CircularCalculator, Circular);
    calculates_nothing!(Cycling, Cycled);

    #[test]
    fn calculators_requiring_each_other_cannot_be_ordered() {
        let mut plugins = phase(
            RoundRobin,
            Vec::new(),
            vec![Box::new(CircularCalculator), Box::new(Cycling)],
        );

        assert_eq!(
            plugins.order_metrics(),
            Err(Incompatibility::CyclicRequirements {
                plugins: vec![format!("{CircularCalculator:?}"), format!("{Cycling:?}")],
            })
        );
        assert!(matches!(plugins.check_order(), Err(OrderError::Cyclic(_))));
    }
}
