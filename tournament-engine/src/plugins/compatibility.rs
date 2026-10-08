use std::{
    any::{Any, TypeId, type_name},
    fmt::Debug,
};

use thiserror::Error;

use crate::{
    map::Map,
    plugins::{
        metrics_calculator::{CalculatedMetric, MetricId, MetricsCalculator},
        phase_format::PhaseFormat,
        ranker::Ranker,
    },
};

/// Something true of a phase, which the plugins of the phase and its context provide and require,
/// see [`Declaration`].
///
/// Every `'static` type with a [`Debug`] impl is a fact, identified by its type. A fact may carry a
/// value, so that a requirement can state a condition on it.
pub trait Fact: Any + Debug {}

impl<T: Any + Debug> Fact for T {}

/// What a plugin provides to and requires of the phase it is part of, filled in by the `declare`
/// method of its axis trait.
///
/// A declaration has to depend on the plugin's configuration alone, since it is made before the
/// phase exists.
pub struct Declaration {
    provided: Vec<ProvidedFact>,
    required: Vec<RequiredFact>,
    required_metrics: Vec<RequiredMetric>,
}

struct ProvidedFact {
    type_id: TypeId,
    name: &'static str,
    value: Box<dyn Fact>,
}

struct RequiredFact {
    type_id: TypeId,
    name: &'static str,
    condition: Option<Condition>,
}

struct Condition {
    description: &'static str,
    holds: Holds,
}

/// A condition on a fact, given the fact as [`Any`].
type Holds = Box<dyn Fn(&dyn Any) -> bool>;

struct RequiredMetric {
    id: MetricId,
    name: &'static str,
    default_calculator: fn() -> Option<Box<dyn MetricsCalculator>>,
}

impl Declaration {
    pub(crate) fn new() -> Self {
        Self {
            provided: Vec::new(),
            required: Vec::new(),
            required_metrics: Vec::new(),
        }
    }

    /// The declaration `declare` makes.
    pub(crate) fn of(declare: impl FnOnce(&mut Self)) -> Self {
        let mut declaration = Self::new();
        declare(&mut declaration);
        declaration
    }

    /// Declares that `fact` holds for the phase.
    ///
    /// No two plugins of a phase may provide the same fact. A fact of the phase's context is
    /// replaced, though.
    pub fn provides<F: Fact>(&mut self, fact: F) -> &mut Self {
        self.provided.push(ProvidedFact {
            type_id: TypeId::of::<F>(),
            name: type_name::<F>(),
            value: Box::new(fact),
        });
        self
    }

    /// Declares that the phase must have a fact of type `F`, whatever its value.
    pub fn requires<F: Fact>(&mut self) -> &mut Self {
        self.required.push(RequiredFact {
            type_id: TypeId::of::<F>(),
            name: type_name::<F>(),
            condition: None,
        });
        self
    }

    /// Declares that the phase must have a fact of type `F` for which `holds` is true, where
    /// `condition` describes what `holds` checks for error messages, e.g. "rankings of single
    /// players".
    pub fn requires_that<F: Fact>(
        &mut self,
        condition: &'static str,
        holds: fn(&F) -> bool,
    ) -> &mut Self {
        self.required.push(RequiredFact {
            type_id: TypeId::of::<F>(),
            name: type_name::<F>(),
            condition: Some(Condition {
                description: condition,
                holds: Box::new(move |fact| {
                    holds(fact.downcast_ref().expect("facts are keyed by their type"))
                }),
            }),
        });
        self
    }

    /// Declares that the phase must calculate metric `M`.
    ///
    /// A metrics calculator that requires a metric runs after the calculator of that metric. If
    /// the phase configures no calculator for `M`, the
    /// [default calculator](CalculatedMetric::default_calculator) of `M` is added.
    pub fn requires_metric<M: CalculatedMetric>(&mut self) -> &mut Self {
        self.required_metrics.push(RequiredMetric {
            id: TypeId::of::<M>(),
            name: type_name::<M>(),
            default_calculator: M::default_calculator,
        });
        self
    }
}

/// Why plugins cannot be combined into one phase.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Incompatibility {
    #[error("{first} and {second} calculate the same metric")]
    MetricCalculatedTwice { first: String, second: String },
    #[error(
        "{required_by} requires the metric {metric}, which no calculator of the phase calculates \
         and which has no default calculator"
    )]
    MetricNotCalculated {
        metric: &'static str,
        required_by: String,
    },
    #[error("the metrics calculators {calculators:?} require each other's metrics in a cycle")]
    CyclicMetricRequirements { calculators: Vec<String> },
    #[error("{calculator} runs before the calculator of a metric it requires")]
    MetricsCalculatorsOutOfOrder { calculator: String },
    #[error("{first} and {second} both provide {fact}")]
    FactProvidedTwice {
        fact: &'static str,
        first: String,
        second: String,
    },
    #[error("{required_by} requires {fact}, which nothing provides")]
    FactNotProvided {
        fact: &'static str,
        required_by: String,
    },
    #[error("{required_by} requires {condition}, but the phase has {provided}")]
    ConditionNotMet {
        condition: &'static str,
        required_by: String,
        provided: String,
    },
}

/// The metric `calculator` calculates.
fn metric_of(calculator: &dyn MetricsCalculator) -> MetricId {
    (&*calculator.default_metric_value() as &dyn Any).type_id()
}

/// The metrics calculators of a phase with format `phase_format` and ranker `ranker`, given that
/// `calculators` are the ones configured, put into an order in which every calculator runs after
/// the calculators of the metrics it requires, keeping the configured order otherwise.
///
/// With `add_defaults` the default calculator of every required metric no calculator calculates is
/// added, otherwise such a metric is an error, and so are configured calculators that are not
/// already ordered.
pub(crate) fn resolve_metrics_calculators(
    phase_format: &dyn PhaseFormat,
    ranker: &dyn Ranker,
    mut calculators: Vec<Box<dyn MetricsCalculator>>,
    add_defaults: bool,
) -> Result<Vec<Box<dyn MetricsCalculator>>, Incompatibility> {
    let mut calculated: Map<MetricId, usize> = Map::default();
    for (index, calculator) in calculators.iter().enumerate() {
        if let Some(&first) = calculated.get(&metric_of(&**calculator)) {
            return Err(Incompatibility::MetricCalculatedTwice {
                first: format!("{:?}", calculators[first]),
                second: format!("{calculator:?}"),
            });
        }
        calculated.insert(metric_of(&**calculator), index);
    }

    // the metrics required, together with who requires them, that are not yet known to be
    // calculated
    let mut unresolved: Vec<(String, RequiredMetric)> = [
        (
            format!("{phase_format:?}"),
            Declaration::of(|declaration| phase_format.declare(declaration)),
        ),
        (
            format!("{ranker:?}"),
            Declaration::of(|declaration| ranker.declare(declaration)),
        ),
    ]
    .into_iter()
    .flat_map(|(required_by, declaration)| {
        declaration
            .required_metrics
            .into_iter()
            .map(move |metric| (required_by.clone(), metric))
    })
    .collect();
    // the metrics each calculator requires, by the index of the calculator; grows with
    // `calculators`, which grows by the defaults added, which may require metrics themselves
    let mut required_by_calculator: Vec<Vec<MetricId>> = Vec::new();
    loop {
        for (required_by, metric) in unresolved.drain(..) {
            if calculated.contains_key(&metric.id) {
                continue;
            }
            let default = add_defaults
                .then(metric.default_calculator)
                .flatten()
                .ok_or(Incompatibility::MetricNotCalculated {
                    metric: metric.name,
                    required_by,
                })?;
            assert_eq!(
                metric_of(&*default),
                metric.id,
                "the default calculator of {} must calculate it",
                metric.name
            );
            calculated.insert(metric.id, calculators.len());
            calculators.push(default);
        }
        let Some(calculator) = calculators.get(required_by_calculator.len()) else {
            break;
        };
        let declaration = Declaration::of(|declaration| calculator.declare(declaration));
        required_by_calculator.push(
            declaration
                .required_metrics
                .iter()
                .map(|metric| metric.id)
                .collect(),
        );
        let required_by = format!("{calculator:?}");
        unresolved.extend(
            declaration
                .required_metrics
                .into_iter()
                .map(|metric| (required_by.clone(), metric)),
        );
    }

    // every required metric is calculated by now, so it maps to the index of its calculator
    let dependencies: Vec<Vec<usize>> = required_by_calculator
        .iter()
        .enumerate()
        .map(|(index, metrics)| {
            metrics
                .iter()
                .map(|metric| calculated[metric])
                // a calculator reading its own metric reads the value it calculated before
                .filter(|&dependency| dependency != index)
                .collect()
        })
        .collect();
    // a topological sort that always takes the first calculator in configured order whose
    // dependencies all run before it, so that an order that is already valid stays as it is
    let mut placed = vec![false; calculators.len()];
    let mut order = Vec::with_capacity(calculators.len());
    while order.len() < calculators.len() {
        let Some(next) = (0..calculators.len()).find(|&index| {
            !placed[index]
                && dependencies[index]
                    .iter()
                    .all(|&dependency| placed[dependency])
        }) else {
            return Err(Incompatibility::CyclicMetricRequirements {
                calculators: (0..calculators.len())
                    .filter(|&index| !placed[index])
                    .map(|index| format!("{:?}", calculators[index]))
                    .collect(),
            });
        };
        if !add_defaults && next != order.len() {
            return Err(Incompatibility::MetricsCalculatorsOutOfOrder {
                calculator: format!("{:?}", calculators[order.len()]),
            });
        }
        placed[next] = true;
        order.push(next);
    }

    let mut calculators: Vec<_> = calculators.into_iter().map(Some).collect();
    Ok(order
        .into_iter()
        .map(|index| {
            calculators[index]
                .take()
                .expect("an order holds every index once")
        })
        .collect())
}

/// Checks that every fact required in `declarations` is provided by another one of them or by
/// `context`, and meets the condition it is required with, where each declaration comes with the
/// name of the plugin that made it.
pub(crate) fn check_facts(
    context: Declaration,
    declarations: &[(String, Declaration)],
) -> Result<(), Incompatibility> {
    // every provided fact by its type, together with who provides it or `None` for the context
    let mut facts: Map<TypeId, (Option<&str>, &ProvidedFact)> = Map::default();
    for fact in &context.provided {
        facts.insert(fact.type_id, (None, fact));
    }
    for (provider, declaration) in declarations {
        for fact in &declaration.provided {
            if let Some((Some(first), _)) = facts.get(&fact.type_id) {
                return Err(Incompatibility::FactProvidedTwice {
                    fact: fact.name,
                    first: (*first).to_owned(),
                    second: provider.clone(),
                });
            }
            facts.insert(fact.type_id, (Some(provider.as_str()), fact));
        }
    }

    for (required_by, declaration) in declarations {
        for requirement in &declaration.required {
            let Some((_, fact)) = facts.get(&requirement.type_id) else {
                return Err(Incompatibility::FactNotProvided {
                    fact: requirement.name,
                    required_by: required_by.clone(),
                });
            };
            if let Some(condition) = &requirement.condition
                && !(condition.holds)(&*fact.value as &dyn Any)
            {
                return Err(Incompatibility::ConditionNotMet {
                    condition: condition.description,
                    required_by: required_by.clone(),
                    provided: format!("{:?}", fact.value),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::slice;

    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{
        entities::competition::{Bye, Match, MatchResult, PhaseData, RankingId},
        plugins::{
            metrics_calculator::Metric, phase_format::round_robin::RoundRobin,
            ranker::HierarchicalRanker,
        },
        type_map::TypeMapValue,
    };

    /// A metric no calculator of which reads another metric, with a default calculator.
    #[derive(Debug, Default, Clone, Serialize, Deserialize)]
    struct Base;

    #[typetag::serde(name = "tournament_engine::compatibility::tests::base")]
    impl TypeMapValue for Base {}

    impl CalculatedMetric for Base {
        fn default_calculator() -> Option<Box<dyn MetricsCalculator>> {
            Some(Box::new(Calculator::new(Name::Base, &[])))
        }
    }

    /// A metric derived from [`Base`], whose default calculator requires it.
    #[derive(Debug, Default, Clone, Serialize, Deserialize)]
    struct Derived;

    #[typetag::serde(name = "tournament_engine::compatibility::tests::derived")]
    impl TypeMapValue for Derived {}

    impl CalculatedMetric for Derived {
        fn default_calculator() -> Option<Box<dyn MetricsCalculator>> {
            Some(Box::new(Calculator::new(Name::Derived, &[Name::Base])))
        }
    }

    /// A metric without a default calculator.
    #[derive(Debug, Default, Clone, Serialize, Deserialize)]
    struct Configured;

    #[typetag::serde(name = "tournament_engine::compatibility::tests::configured")]
    impl TypeMapValue for Configured {}

    impl CalculatedMetric for Configured {}

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    enum Name {
        Base,
        Derived,
        Configured,
    }

    /// A calculator of the metric `calculates` that requires the metrics `requires` and never
    /// updates anything.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct Calculator {
        calculates: Name,
        requires: Vec<Name>,
    }

    impl Calculator {
        fn new(calculates: Name, requires: &[Name]) -> Self {
            Self {
                calculates,
                requires: requires.to_vec(),
            }
        }
    }

    #[typetag::serde(name = "tournament_engine::compatibility::tests::calculator")]
    impl MetricsCalculator for Calculator {
        fn declare(&self, declaration: &mut Declaration) {
            for name in &self.requires {
                match name {
                    Name::Base => declaration.requires_metric::<Base>(),
                    Name::Derived => declaration.requires_metric::<Derived>(),
                    Name::Configured => declaration.requires_metric::<Configured>(),
                };
            }
        }

        fn default_metric_value(&self) -> Metric {
            match self.calculates {
                Name::Base => Box::new(Base),
                Name::Derived => Box::new(Derived),
                Name::Configured => Box::new(Configured),
            }
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

    /// The resolved `calculators` of a phase whose other plugins require nothing, each given by its
    /// [`Debug`] form, since a boxed calculator cannot be downcast.
    fn resolve(
        calculators: &[Calculator],
        add_defaults: bool,
    ) -> Result<Vec<String>, Incompatibility> {
        let calculators = calculators
            .iter()
            .map(|calculator| Box::new(calculator.clone()) as Box<dyn MetricsCalculator>)
            .collect();
        let resolved = resolve_metrics_calculators(
            &RoundRobin,
            &HierarchicalRanker::new(Vec::new()),
            calculators,
            add_defaults,
        )?;
        Ok(resolved
            .iter()
            .map(|calculator| format!("{calculator:?}"))
            .collect())
    }

    /// The [`Debug`] forms of `calculators`, to compare with what [`resolve`] returns.
    fn debug(calculators: &[Calculator]) -> Vec<String> {
        calculators
            .iter()
            .map(|calculator| format!("{calculator:?}"))
            .collect()
    }

    #[test]
    fn calculators_without_requirements_keep_their_order() {
        let calculators = [
            Calculator::new(Name::Configured, &[]),
            Calculator::new(Name::Base, &[]),
        ];

        assert_eq!(resolve(&calculators, true), Ok(debug(&calculators)));
    }

    #[test]
    fn a_calculator_runs_after_the_calculators_of_the_metrics_it_requires() {
        let derived = Calculator::new(Name::Derived, &[Name::Base]);
        let base = Calculator::new(Name::Base, &[]);

        assert_eq!(
            resolve(&[derived.clone(), base.clone()], true),
            Ok(debug(&[base, derived]))
        );
    }

    #[test]
    fn missing_calculators_are_added_from_the_defaults_of_their_metrics_recursively() {
        let configured = Calculator::new(Name::Configured, &[Name::Derived]);

        assert_eq!(
            resolve(slice::from_ref(&configured), true),
            Ok(debug(&[
                Calculator::new(Name::Base, &[]),
                Calculator::new(Name::Derived, &[Name::Base]),
                configured,
            ]))
        );
    }

    #[test]
    fn a_configured_calculator_wins_over_the_default() {
        // the default calculator of `Derived` requires `Base`, this one does not
        let derived = Calculator::new(Name::Derived, &[]);
        let configured = Calculator::new(Name::Configured, &[Name::Derived]);

        assert_eq!(
            resolve(&[configured.clone(), derived.clone()], true),
            Ok(debug(&[derived, configured]))
        );
    }

    #[test]
    fn a_required_metric_without_calculator_and_default_is_an_error() {
        let base = Calculator::new(Name::Base, &[Name::Configured]);

        assert_eq!(
            resolve(slice::from_ref(&base), true),
            Err(Incompatibility::MetricNotCalculated {
                metric: type_name::<Configured>(),
                required_by: format!("{base:?}"),
            })
        );
    }

    #[test]
    fn two_calculators_of_the_same_metric_are_an_error() {
        let first = Calculator::new(Name::Base, &[]);
        let second = Calculator::new(Name::Base, &[Name::Derived]);

        assert_eq!(
            resolve(&[first.clone(), second.clone()], true),
            Err(Incompatibility::MetricCalculatedTwice {
                first: format!("{first:?}"),
                second: format!("{second:?}"),
            })
        );
    }

    #[test]
    fn calculators_requiring_each_other_are_an_error() {
        let base = Calculator::new(Name::Base, &[Name::Derived]);
        let derived = Calculator::new(Name::Derived, &[Name::Base]);

        assert_eq!(
            resolve(&[base.clone(), derived.clone()], true),
            Err(Incompatibility::CyclicMetricRequirements {
                calculators: vec![format!("{base:?}"), format!("{derived:?}")],
            })
        );
    }

    #[test]
    fn a_calculator_may_require_its_own_metric() {
        let base = Calculator::new(Name::Base, &[Name::Base]);

        assert_eq!(resolve(slice::from_ref(&base), true), Ok(debug(&[base])));
    }

    #[test]
    fn without_defaults_a_valid_order_is_accepted() {
        let calculators = [
            Calculator::new(Name::Base, &[]),
            Calculator::new(Name::Derived, &[Name::Base]),
        ];

        assert_eq!(resolve(&calculators, false), Ok(debug(&calculators)));
    }

    #[test]
    fn without_defaults_a_missing_calculator_is_an_error() {
        let derived = Calculator::new(Name::Derived, &[Name::Base]);

        assert_eq!(
            resolve(slice::from_ref(&derived), false),
            Err(Incompatibility::MetricNotCalculated {
                metric: type_name::<Base>(),
                required_by: format!("{derived:?}"),
            })
        );
    }

    #[test]
    fn without_defaults_an_unsorted_order_is_an_error() {
        let derived = Calculator::new(Name::Derived, &[Name::Base]);
        let base = Calculator::new(Name::Base, &[]);

        assert_eq!(
            resolve(&[derived.clone(), base], false),
            Err(Incompatibility::MetricsCalculatorsOutOfOrder {
                calculator: format!("{derived:?}"),
            })
        );
    }

    #[derive(Debug, PartialEq, Eq)]
    struct EntrantsPerRanking(u32);

    #[derive(Debug)]
    struct Annotated;

    fn declared(name: &str, declare: impl FnOnce(&mut Declaration)) -> (String, Declaration) {
        (name.to_owned(), Declaration::of(declare))
    }

    #[test]
    fn a_fact_provided_by_another_plugin_meets_its_requirement() {
        let declarations = [
            declared("format", |declaration| {
                declaration.provides(Annotated);
            }),
            declared("calculator", |declaration| {
                declaration.requires::<Annotated>();
            }),
        ];

        assert_eq!(check_facts(Declaration::new(), &declarations), Ok(()));
    }

    #[test]
    fn plugins_may_require_facts_of_each_other() {
        let declarations = [
            declared("format", |declaration| {
                declaration
                    .provides(Annotated)
                    .requires::<EntrantsPerRanking>();
            }),
            declared("ranker", |declaration| {
                declaration
                    .provides(EntrantsPerRanking(1))
                    .requires::<Annotated>();
            }),
        ];

        assert_eq!(check_facts(Declaration::new(), &declarations), Ok(()));
    }

    #[test]
    fn a_fact_nothing_provides_is_an_error() {
        let declarations = [declared("calculator", |declaration| {
            declaration.requires::<Annotated>();
        })];

        assert_eq!(
            check_facts(Declaration::new(), &declarations),
            Err(Incompatibility::FactNotProvided {
                fact: type_name::<Annotated>(),
                required_by: "calculator".to_owned(),
            })
        );
    }

    #[test]
    fn a_fact_provided_by_two_plugins_is_an_error() {
        let declarations = [
            declared("format", |declaration| {
                declaration.provides(Annotated);
            }),
            declared("ranker", |declaration| {
                declaration.provides(Annotated);
            }),
        ];

        assert_eq!(
            check_facts(Declaration::new(), &declarations),
            Err(Incompatibility::FactProvidedTwice {
                fact: type_name::<Annotated>(),
                first: "format".to_owned(),
                second: "ranker".to_owned(),
            })
        );
    }

    fn requires_single_players(declaration: &mut Declaration) {
        declaration
            .requires_that::<EntrantsPerRanking>("rankings of single players", |fact| fact.0 == 1);
    }

    #[test]
    fn a_condition_is_checked_against_the_fact_of_the_context() {
        let context = Declaration::of(|declaration| {
            declaration.provides(EntrantsPerRanking(1));
        });
        let declarations = [declared("format", requires_single_players)];

        assert_eq!(check_facts(context, &declarations), Ok(()));
    }

    #[test]
    fn a_plugin_replaces_a_fact_of_the_context() {
        let context = Declaration::of(|declaration| {
            declaration.provides(EntrantsPerRanking(1));
        });
        let declarations = [
            declared("draw", |declaration| {
                declaration.provides(EntrantsPerRanking(2));
            }),
            declared("format", requires_single_players),
        ];

        assert_eq!(
            check_facts(context, &declarations),
            Err(Incompatibility::ConditionNotMet {
                condition: "rankings of single players",
                required_by: "format".to_owned(),
                provided: format!("{:?}", EntrantsPerRanking(2)),
            })
        );
    }
}
