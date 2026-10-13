//! The plugin axes of a phase and the plugins of this crate, built on `plugin_factory`.
//!
//! A phase is played in one [`PhaseFormat`], ranked by a list of [`SliceRanker`]s and calculates
//! the metrics of a list of [`MetricsCalculator`]s; [`PhasePlugins`] is that composite. Every
//! plugin registers itself in the catalog of its axis, see [`plugin_factory::catalog`], so a
//! consumer configuring a phase finds them there, and declares what it provides to and requires of
//! its phase, see [`facts`] and [`plugin_factory::compatibility`].
//!
//! # Labelling a plugin
//!
//! A consumer shows a plugin and its fields by their labels and descriptions alone, in lists of
//! options, in questions (`random rounds: rounds`) and in summaries (`round robin, ranked by more
//! points, then more Sonneborn-Berger`; `single elimination (match for third place: yes)`). A
//! plugin of any crate is shown next to the plugins of this one, so it should be labelled the same
//! way:
//!
//! - **Labels are lowercase noun phrases** without a final period, written as they read in the
//!   middle of a sentence. Proper names and established abbreviations keep their case
//!   (`Monster DYP`, `Buchholz`). A consumer capitalizes a label where a sentence starts with it.
//!   Labels contain no commas, since consumers list them separated by commas.
//! - **A format is labelled by the name of the format** (`round robin`, `double elimination`).
//! - **A metrics calculator is labelled by the metric it calculates**, as a column of a ranking
//!   would be named (`points`, `win-draw-loss record`), not by what it does.
//! - **A criterion states which way it sorts**: a comparative followed by what it compares, so that
//!   `ranked by <label>` reads as a sentence (`more points`, `fewer losses`, `later first loss`).
//!   Two criteria of one metric are told apart by their labels alone.
//! - **A field is labelled by what its answer is**, so that `<label>: <answer>` reads naturally
//!   (`rounds: 5`, `match for third place: yes`, `points for a win: 1`), not as a question or a
//!   command (not `Play a match for third place?`). A consumer prefixes it with the label of its
//!   plugin, so it does not repeat that.
//! - **A description says what the label leaves open**, as a lowercase phrase without a final
//!   period: what a format does, what a metric counts, how a criterion treats its extremes
//!   (`never having lost is best`), what an answer means. Leave it out rather than restate the
//!   label (no `more points are better` for `more points`).
//! - **A field offers an answer**, with `#[plugin(default = ..)]`, wherever one value suits most
//!   phases, e.g. the scoring of chess for points.

use plugin_factory::{
    draft::{self, Draft, Issue},
    schema::{Configurable, DependencyOrderedSet, DependencyOrderedSetError},
};
use serde::{Deserialize, Serialize};

use crate::{
    entities::competition::Competition,
    plugins::{
        metrics_calculator::MetricsCalculator,
        phase_format::PhaseFormat,
        ranker::{HierarchicalRanker, SliceRanker},
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
///
/// The calculators are a [`DependencyOrderedSet`]: a phase calculates each metric once, and the
/// set holds them in the order they run in, each after the calculators of the metrics it reads.
#[derive(Debug, Clone, Serialize, Deserialize, Configurable)]
pub struct PhasePlugins {
    #[plugin(label = "format", description = "how the matches are scheduled")]
    format: Box<dyn PhaseFormat>,
    #[serde(default)]
    #[plugin(
        label = "ranked by",
        description = "best first, every criterion only breaks the ties of the ones before it"
    )]
    ranking: Vec<Box<dyn SliceRanker>>,
    #[serde(default)]
    #[plugin(label = "metrics", description = "calculated for every team")]
    metrics: DependencyOrderedSet<dyn MetricsCalculator>,
}

impl PhasePlugins {
    /// The plugins of a phase played in `format`, ranked by `ranking`, best first, and calculating
    /// the metrics of `metrics`, put into the order they run in, see [`DependencyOrderedSet`].
    ///
    /// # Errors
    /// Returns an error if `metrics` holds a calculator twice, or calculators that require facts
    /// of each other in a cycle, see [`DependencyOrderedSet::new`].
    pub fn new(
        format: Box<dyn PhaseFormat>,
        ranking: Vec<Box<dyn SliceRanker>>,
        metrics: Vec<Box<dyn MetricsCalculator>>,
    ) -> Result<Self, DependencyOrderedSetError> {
        Ok(Self {
            format,
            ranking,
            metrics: DependencyOrderedSet::new(metrics)?,
        })
    }

    pub fn format(&self) -> &dyn PhaseFormat {
        &*self.format
    }

    /// The slice rankers, best first.
    pub fn ranking(&self) -> &[Box<dyn SliceRanker>] {
        &self.ranking
    }

    /// The metrics calculators, in the order they run in.
    pub fn metrics(&self) -> &DependencyOrderedSet<dyn MetricsCalculator> {
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

    /// The format, the ranker and the metrics calculators a phase is run with.
    pub(crate) fn into_parts(self) -> (Box<dyn PhaseFormat>, HierarchicalRanker, Metrics) {
        (
            self.format,
            HierarchicalRanker::new(self.ranking),
            self.metrics.into_iter().collect(),
        )
    }
}

/// The metrics calculators of a phase, in the order they run in.
pub(crate) type Metrics = Vec<Box<dyn MetricsCalculator>>;

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
        Plugin, PluginType,
        catalog::AnyAxis,
        compatibility::{self, Declaration, Fact, Incompatibility},
        config::PluginConfig,
        draft::Problem,
        register,
    };

    use super::*;
    use crate::{
        entities::competition::{Bye, Match, MatchResult, PhaseData, RankingId},
        map::Map,
        plugins::{
            facts::{CalculatesMetric, EntrantsPerRanking, MatchAnnotation},
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
        PhasePlugins::new(Box::new(format), ranking, metrics).unwrap()
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
    fn provides<T: Fact + Default>(plugin: &dyn Plugin) -> bool {
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

    /// `plugins` settled in a competition, as a consumer adding them would.
    fn settled(plugins: &PhasePlugins) -> Result<PhasePlugins, Vec<Issue>> {
        plugins.settle(&Competition::default())
    }

    /// What can be checked of the labelling guidelines of the module, for every plugin registered
    /// and the fields of the composite.
    #[test]
    fn every_plugin_is_labelled_as_the_guidelines_say() {
        fn check_text(text: &str, what: &str) {
            assert!(
                !text.is_empty()
                    && text.trim() == text
                    && !text.ends_with(['.', '?', '!'])
                    && !text.starts_with(|c: char| c.is_uppercase()),
                "{what} {text:?} is no lowercase phrase without a final period"
            );
        }

        let axes = [
            ("format", AnyAxis::of::<dyn PhaseFormat>()),
            ("criterion", AnyAxis::of::<dyn SliceRanker>()),
            ("calculator", AnyAxis::of::<dyn MetricsCalculator>()),
        ];
        let mut fields: Vec<_> = PhasePlugins::schema().fields().to_vec();
        for (axis_name, axis) in axes {
            let mut labels = Vec::new();
            for entry in axis.entries() {
                let label = entry.label();
                assert!(
                    !label.is_empty() && label.trim() == label && !label.contains([',', '.']),
                    "the label of {} is {label:?}",
                    entry.id()
                );
                assert!(
                    !labels.contains(&label),
                    "two {axis_name}s are labelled {label:?}"
                );
                labels.push(label);
                if let Some(description) = entry.description() {
                    check_text(description, "the description");
                }
                if axis_name == "criterion" {
                    let comparative = label.split(' ').next().unwrap();
                    assert!(
                        [
                            "more", "fewer", "less", "later", "earlier", "higher", "lower"
                        ]
                        .contains(&comparative),
                        "the criterion {label:?} does not say which way it sorts"
                    );
                }
                fields.extend(entry.schema().fields().iter().cloned());
            }
        }
        for field in fields {
            check_text(field.label(), "the field label");
            if let Some(description) = field.description() {
                check_text(description, "the description");
            }
        }
    }

    #[test]
    fn a_calculator_runs_after_the_calculators_of_the_metrics_it_reads() {
        // listed so that every calculator reading a metric comes before the one calculating it
        let plugins = phase(
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
    }

    /// A calculator is the same plugin whatever it is configured with, so a phase cannot score
    /// its points two ways.
    #[test]
    fn a_phase_takes_a_calculator_once() {
        let built = PhasePlugins::new(
            Box::new(RoundRobin),
            Vec::new(),
            vec![
                Box::new(ONE_HALF),
                Box::new(WinsDrawsLossesCalculator),
                Box::new(PointsCalculator::new(3.0, 1.0)),
            ],
        );

        assert_eq!(
            built.err(),
            Some(DependencyOrderedSetError::Duplicate { position: 2 })
        );
    }

    /// A phase saved in an order its calculators cannot run in, e.g. edited by hand, is put into
    /// running order when it is loaded, while a saved valid order stays as it is.
    #[test]
    fn deserialized_calculators_are_put_into_running_order() {
        let ordered = phase(
            RoundRobin,
            Vec::new(),
            vec![Box::new(WinsDrawsLossesCalculator), Box::new(ONE_HALF)],
        );
        let mut saved = serde_json::to_value(&ordered).unwrap();
        saved["metrics"].as_array_mut().unwrap().reverse();

        let loaded: PhasePlugins = serde_json::from_value(saved).unwrap();

        assert_eq!(metrics(&loaded), metrics![WinsDrawsLosses, Points]);
        let reloaded: PhasePlugins =
            serde_json::from_value(serde_json::to_value(&loaded).unwrap()).unwrap();
        assert_eq!(metrics(&reloaded), metrics(&loaded));
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

        // the added calculators are put into running order as well
        assert_eq!(
            metrics(&settled),
            metrics![WinsDrawsLosses, Points, Matches, SonnebornBerger]
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
            // the type of a metric behind a `dyn` is only known by its derived `Debug` form, which
            // starts with the name of the type
            let metric: String = format!("{:?}", calculator.default_metric_value())
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();

            // a calculator providing its metric twice conflicts with itself about that metric
            let Some(Incompatibility::FactProvidedTwice { fact, .. }) =
                compatibility::check(|_| {}, &both).err()
            else {
                panic!("{} does not provide a fact only once", entry.id());
            };
            assert!(
                fact.starts_with("CalculatesMetric<") && fact.ends_with(&format!("::{metric}>")),
                "{} provides {fact} but calculates {metric}",
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
                .provide(CalculatesMetric::<Circular>::new())
                .require::<CalculatesMetric<Cycled>>("the cycled metric");
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
                .provide(CalculatesMetric::<Cycled>::new())
                .require::<CalculatesMetric<Circular>>("the circular metric");
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
        let built = PhasePlugins::new(
            Box::new(RoundRobin),
            Vec::new(),
            vec![Box::new(CircularCalculator), Box::new(Cycling)],
        );
        let saved = serde_json::json!({
            "format": {"type": RoundRobin::ID, "value": null},
            "ranking": [],
            "metrics": [
                {"type": CircularCalculator::ID, "value": null},
                {"type": Cycling::ID, "value": null},
            ],
        });

        assert_eq!(
            built.err(),
            Some(DependencyOrderedSetError::Cyclic(
                Incompatibility::CyclicRequirements {
                    plugins: vec![format!("{CircularCalculator:?}"), format!("{Cycling:?}")],
                }
            ))
        );
        assert!(serde_json::from_value::<PhasePlugins>(saved).is_err());
    }
}
