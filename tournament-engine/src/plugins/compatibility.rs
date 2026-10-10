use std::{
    any::{Any, TypeId, type_name},
    fmt::{self, Debug, Formatter},
    marker::PhantomData,
    num::NonZeroU32,
};

use thiserror::Error;

use crate::{map::Map, plugins::Plugin, type_map::TypeMapValue};

/// Something true of a phase, which the plugins of the phase and its context provide and require,
/// see [`Declaration`].
///
/// Every `'static` type with a [`Debug`] impl is a fact, identified by its type. A fact may carry a
/// value, so that a requirement can state a condition on it.
pub trait Fact: Any + Debug {}

impl<T: Any + Debug> Fact for T {}

/// How many entrants every ranking of the phase stands for: one for rankings of single players,
/// two for a phase played between pairs.
///
/// Provided by the phase's context, so a plugin only provides it if it changes the rankings of its
/// own phase, e.g. a draw that pairs the entrants up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntrantsPerRanking(pub NonZeroU32);

impl EntrantsPerRanking {
    /// One entrant per ranking, i.e. rankings of single players.
    pub const SINGLE: Self = Self(NonZeroU32::MIN);
}

/// Every match the phase plays that `A` applies to carries it as an annotation.
///
/// Provided by the phase format writing the annotation, and required by everything reading it,
/// e.g. the calculator of the [round of elimination] requires `MatchAnnotation<Eliminates>`. Which
/// matches `A` applies to is up to the format: a double elimination annotates only the matches that
/// can knock an entrant out with `Eliminates`.
///
/// [round of elimination]: crate::plugins::metrics_calculator::round_of_elimination
pub struct MatchAnnotation<A: TypeMapValue>(PhantomData<A>);

impl<A: TypeMapValue> MatchAnnotation<A> {
    /// The fact that the matches of the phase carry an `A` annotation.
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<A: TypeMapValue> Default for MatchAnnotation<A> {
    fn default() -> Self {
        Self::new()
    }
}

// written out rather than derived, which would print the `PhantomData` and demand `A: Debug`
impl<A: TypeMapValue> Debug for MatchAnnotation<A> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "MatchAnnotation<{}>", type_name::<A>())
    }
}

/// What a plugin provides to and requires of the phase it is part of, filled in by
/// [`Plugin::declare`].
pub struct Declaration {
    provided: Vec<ProvidedFact>,
    required: Vec<RequiredFact>,
}

struct ProvidedFact {
    type_id: TypeId,
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

impl Declaration {
    pub(crate) fn new() -> Self {
        Self {
            provided: Vec::new(),
            required: Vec::new(),
        }
    }

    /// The declaration `declare` makes.
    pub(crate) fn of(declare: impl FnOnce(&mut Self)) -> Self {
        let mut declaration = Self::new();
        declare(&mut declaration);
        declaration
    }

    /// Whether this declaration provides a fact of the type `type_id`.
    pub(crate) fn provides_type(&self, type_id: TypeId) -> bool {
        self.provided.iter().any(|fact| fact.type_id == type_id)
    }

    /// Declares that `fact` holds for the phase.
    ///
    /// No two plugins of a phase may provide the same fact. A fact of the phase's context is
    /// replaced, though.
    pub fn provides<F: Fact>(&mut self, fact: F) -> &mut Self {
        self.provided.push(ProvidedFact {
            type_id: TypeId::of::<F>(),
            value: Box::new(fact),
        });
        self
    }

    /// Declares that the phase must have a fact of type `F`, whatever its value.
    ///
    /// A phase that has nothing providing it gets the one plugin the
    /// [catalog](crate::plugins::catalog) registers for it, if there is exactly one and it needs no
    /// configuration.
    pub fn requires<F: Fact>(&mut self) -> &mut Self {
        self.require::<F>(None)
    }

    /// Declares that the phase must have a fact of type `F` for which `holds` is true, where
    /// `condition` describes what `holds` checks for error messages, e.g. "rankings of single
    /// players".
    pub fn requires_that<F: Fact>(
        &mut self,
        condition: &'static str,
        holds: fn(&F) -> bool,
    ) -> &mut Self {
        let condition = Condition {
            description: condition,
            holds: Box::new(move |fact| {
                holds(fact.downcast_ref().expect("facts are keyed by their type"))
            }),
        };
        self.require::<F>(Some(condition))
    }

    fn require<F: Fact>(&mut self, condition: Option<Condition>) -> &mut Self {
        self.required.push(RequiredFact {
            type_id: TypeId::of::<F>(),
            name: type_name::<F>(),
            condition,
        });
        self
    }
}

/// Why plugins cannot be combined into one phase.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Incompatibility {
    #[error("{first} and {second} both provide {fact}")]
    FactProvidedTwice {
        fact: String,
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
    #[error("{plugins:?} require facts of each other in a cycle")]
    CyclicRequirements { plugins: Vec<String> },
    #[error("{plugin} runs before a plugin providing a fact it requires")]
    OutOfOrder { plugin: String },
}

/// The declaration of `plugin`, together with its name.
pub(crate) fn declaration_of(plugin: &dyn Plugin) -> (String, Declaration) {
    (
        format!("{plugin:?}"),
        Declaration::of(|declaration| plugin.declare(declaration)),
    )
}

/// The types of the facts required in `declarations` that none of them provides, in the order they
/// are required in and with repetitions.
pub(crate) fn unprovided(declarations: &[(String, Declaration)]) -> impl Iterator<Item = TypeId> {
    declarations
        .iter()
        .flat_map(|(_, declaration)| &declaration.required)
        .map(|requirement| requirement.type_id)
        .filter(|&required| {
            !declarations
                .iter()
                .any(|(_, declaration)| declaration.provides_type(required))
        })
}

/// An order in which plugins with `declarations` can run one after another, so that each runs
/// after the plugins providing the facts it requires, given as indices into `declarations`.
/// Plugins keep their given order wherever the requirements allow it, so an order that is
/// already valid stays as it is.
///
/// A plugin may require a fact it provides itself.
///
/// # Errors
/// Returns an error if plugins require facts of each other in a cycle.
pub(crate) fn order(declarations: &[(String, Declaration)]) -> Result<Vec<usize>, Incompatibility> {
    let dependencies: Vec<Vec<usize>> = declarations
        .iter()
        .enumerate()
        .map(|(index, (_, declaration))| {
            (0..declarations.len())
                .filter(|&other| {
                    other != index
                        && declaration.required.iter().any(|requirement| {
                            declarations[other].1.provides_type(requirement.type_id)
                        })
                })
                .collect()
        })
        .collect();
    // a topological sort that always takes the first plugin in the given order whose dependencies
    // all run before it
    let mut placed = vec![false; declarations.len()];
    let mut order = Vec::with_capacity(declarations.len());
    while order.len() < declarations.len() {
        let Some(next) = (0..declarations.len()).find(|&index| {
            !placed[index]
                && dependencies[index]
                    .iter()
                    .all(|&dependency| placed[dependency])
        }) else {
            return Err(Incompatibility::CyclicRequirements {
                plugins: (0..declarations.len())
                    .filter(|&index| !placed[index])
                    .map(|index| declarations[index].0.clone())
                    .collect(),
            });
        };
        placed[next] = true;
        order.push(next);
    }
    Ok(order)
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
                    fact: format!("{:?}", fact.value),
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
            PhasePlugins,
            metrics_calculator::{Metric, MetricsCalculator, MetricsCalculatorKind},
            phase_format::round_robin::RoundRobin,
            ranker::HierarchicalRanker,
        },
        type_map::TypeMapValue,
    };

    /// A metric no calculator of which reads another metric, calculated by a registered calculator
    /// that needs no configuration.
    #[derive(Debug, Default, Clone, Serialize, Deserialize)]
    struct Base;

    #[typetag::serde(name = "tournament_engine::compatibility::tests::base")]
    impl TypeMapValue for Base {}

    inventory::submit! {
        MetricsCalculatorKind::register(
            "tournament_engine::compatibility::tests::base",
            "base",
            "a metric calculated from nothing else",
        )
        .with_default(|| Box::new(Calculator::new(Name::Base, &[])))
    }

    /// A metric derived from [`Base`], whose registered calculator requires it.
    #[derive(Debug, Default, Clone, Serialize, Deserialize)]
    struct Derived;

    #[typetag::serde(name = "tournament_engine::compatibility::tests::derived")]
    impl TypeMapValue for Derived {}

    inventory::submit! {
        MetricsCalculatorKind::register(
            "tournament_engine::compatibility::tests::derived",
            "derived",
            "a metric calculated from the base one",
        )
        .with_default(|| Box::new(Calculator::new(Name::Derived, &[Name::Base])))
    }

    /// A metric whose calculator has to be configured, so that the catalog cannot create it.
    #[derive(Debug, Default, Clone, Serialize, Deserialize)]
    struct Configured;

    #[typetag::serde(name = "tournament_engine::compatibility::tests::configured")]
    impl TypeMapValue for Configured {}

    inventory::submit! {
        MetricsCalculatorKind::register(
            "tournament_engine::compatibility::tests::configured",
            "configured",
            "a metric whose calculator has to be configured",
        )
    }

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

    impl Plugin for Calculator {
        fn declare(&self, declaration: &mut Declaration) {
            match self.calculates {
                Name::Base => declaration.provides(Base),
                Name::Derived => declaration.provides(Derived),
                Name::Configured => declaration.provides(Configured),
            };
            for name in &self.requires {
                match name {
                    Name::Base => declaration.requires::<Base>(),
                    Name::Derived => declaration.requires::<Derived>(),
                    Name::Configured => declaration.requires::<Configured>(),
                };
            }
        }
    }

    #[typetag::serde(name = "tournament_engine::compatibility::tests::calculator")]
    impl MetricsCalculator for Calculator {
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

    /// The plugins of a phase whose format and ranker provide and require nothing, with
    /// `calculators` configured.
    fn plugins(calculators: &[Calculator]) -> PhasePlugins {
        PhasePlugins::new(
            Box::new(RoundRobin),
            Box::new(HierarchicalRanker::new(Vec::new())),
            calculators
                .iter()
                .map(|calculator| Box::new(calculator.clone()) as Box<dyn MetricsCalculator>)
                .collect(),
        )
    }

    /// The [`Debug`] forms of the metrics calculators of `plugins`, since a boxed calculator
    /// cannot be downcast.
    fn names_of_calculators(plugins: &PhasePlugins) -> Vec<String> {
        plugins
            .metrics_calculators()
            .iter()
            .map(|calculator| format!("{calculator:?}"))
            .collect()
    }

    /// The [`Debug`] forms of `calculators`.
    fn names(calculators: &[Calculator]) -> Vec<String> {
        calculators
            .iter()
            .map(|calculator| format!("{calculator:?}"))
            .collect()
    }

    /// The metrics calculators the plugins with `calculators` configured resolve to.
    fn resolve(calculators: &[Calculator]) -> Result<Vec<String>, Incompatibility> {
        let mut plugins = plugins(calculators);
        plugins.resolve()?;
        Ok(names_of_calculators(&plugins))
    }

    #[test]
    fn calculators_without_requirements_keep_their_order() {
        let calculators = [
            Calculator::new(Name::Configured, &[]),
            Calculator::new(Name::Base, &[]),
        ];

        assert_eq!(resolve(&calculators), Ok(names(&calculators)));
    }

    #[test]
    fn a_calculator_runs_after_the_calculators_of_the_metrics_it_requires() {
        let derived = Calculator::new(Name::Derived, &[Name::Base]);
        let base = Calculator::new(Name::Base, &[]);

        assert_eq!(
            resolve(&[derived.clone(), base.clone()]),
            Ok(names(&[base, derived]))
        );
    }

    #[test]
    fn missing_calculators_are_added_from_the_catalog_recursively() {
        let configured = Calculator::new(Name::Configured, &[Name::Derived]);

        assert_eq!(
            resolve(slice::from_ref(&configured)),
            Ok(names(&[
                Calculator::new(Name::Base, &[]),
                Calculator::new(Name::Derived, &[Name::Base]),
                configured,
            ]))
        );
    }

    #[test]
    fn a_configured_calculator_wins_over_the_registered_one() {
        // the registered calculator of `Derived` requires `Base`, this one does not
        let derived = Calculator::new(Name::Derived, &[]);
        let configured = Calculator::new(Name::Configured, &[Name::Derived]);

        assert_eq!(
            resolve(&[configured.clone(), derived.clone()]),
            Ok(names(&[derived, configured]))
        );
    }

    #[test]
    fn a_metric_whose_calculator_has_to_be_configured_is_left_unprovided() {
        let base = Calculator::new(Name::Base, &[Name::Configured]);

        let mut plugins = plugins(slice::from_ref(&base));
        plugins.resolve().unwrap();

        assert_eq!(
            names_of_calculators(&plugins),
            names(slice::from_ref(&base))
        );
        assert_eq!(
            check_facts(Declaration::new(), &plugins.declarations()),
            Err(Incompatibility::FactNotProvided {
                fact: type_name::<Configured>(),
                required_by: format!("{base:?}"),
            })
        );
    }

    #[test]
    fn calculators_requiring_each_other_are_an_error() {
        let base = Calculator::new(Name::Base, &[Name::Derived]);
        let derived = Calculator::new(Name::Derived, &[Name::Base]);

        assert_eq!(
            resolve(&[base.clone(), derived.clone()]),
            Err(Incompatibility::CyclicRequirements {
                plugins: names(&[base, derived]),
            })
        );
    }

    #[test]
    fn a_calculator_may_require_its_own_metric() {
        let base = Calculator::new(Name::Base, &[Name::Base]);

        assert_eq!(resolve(slice::from_ref(&base)), Ok(names(&[base])));
    }

    #[test]
    fn a_resolved_order_passes_the_order_check() {
        let mut plugins = plugins(&[Calculator::new(Name::Configured, &[Name::Derived])]);
        plugins.resolve().unwrap();

        assert_eq!(plugins.check_order(), Ok(()));
    }

    #[test]
    fn an_unsorted_order_fails_the_order_check() {
        let derived = Calculator::new(Name::Derived, &[Name::Base]);
        let base = Calculator::new(Name::Base, &[]);

        assert_eq!(
            plugins(&[derived.clone(), base]).check_order(),
            Err(Incompatibility::OutOfOrder {
                plugin: format!("{derived:?}"),
            })
        );
    }

    #[derive(Debug)]
    struct Annotated;

    /// Two entrants per ranking, i.e. the rankings a draw pairing the entrants up leaves behind.
    const PAIRS: EntrantsPerRanking = EntrantsPerRanking(NonZeroU32::new(2).unwrap());

    fn declared(name: &str, declare: impl FnOnce(&mut Declaration)) -> (String, Declaration) {
        (name.to_owned(), Declaration::of(declare))
    }

    #[test]
    fn plugins_are_ordered_by_any_fact_they_require() {
        let declarations = [
            declared("requiring", |declaration| {
                declaration.requires::<Annotated>();
            }),
            declared("independent", |_| {}),
            declared("annotating", |declaration| {
                declaration.provides(Annotated);
            }),
        ];

        assert_eq!(order(&declarations), Ok(vec![1, 2, 0]));
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
                    .provides(EntrantsPerRanking::SINGLE)
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
                fact: format!("{Annotated:?}"),
                first: "format".to_owned(),
                second: "ranker".to_owned(),
            })
        );
    }

    fn requires_single_players(declaration: &mut Declaration) {
        declaration.requires_that::<EntrantsPerRanking>("rankings of single players", |fact| {
            *fact == EntrantsPerRanking::SINGLE
        });
    }

    #[test]
    fn a_condition_is_checked_against_the_fact_of_the_context() {
        let context = Declaration::of(|declaration| {
            declaration.provides(EntrantsPerRanking::SINGLE);
        });
        let declarations = [declared("format", requires_single_players)];

        assert_eq!(check_facts(context, &declarations), Ok(()));
    }

    #[test]
    fn a_plugin_replaces_a_fact_of_the_context() {
        let context = Declaration::of(|declaration| {
            declaration.provides(EntrantsPerRanking::SINGLE);
        });
        let declarations = [
            declared("draw", |declaration| {
                declaration.provides(PAIRS);
            }),
            declared("format", requires_single_players),
        ];

        assert_eq!(
            check_facts(context, &declarations),
            Err(Incompatibility::ConditionNotMet {
                condition: "rankings of single players",
                required_by: "format".to_owned(),
                provided: format!("{PAIRS:?}"),
            })
        );
    }
}
