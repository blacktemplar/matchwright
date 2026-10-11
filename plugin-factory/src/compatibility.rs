//! What plugins provide to and require of the composite they are part of, and the checks deciding
//! whether they fit together.
//!
//! A plugin states its facts in [`Plugin::static_declare`] and [`Plugin::dyn_declare`].
//! [`check_facts`] decides whether a set of plugins fits together, and [`check_addition`] whether
//! one more plugin may be added to a set that is still being configured, see [`OpenFacts`].

use std::{
    any::{Any, TypeId},
    collections::{BTreeSet, HashMap},
    fmt::Debug,
};

use thiserror::Error;

use crate::Plugin;

/// Something true of a phase, which the plugins of the phase and its context provide and require,
/// see [`Declaration`].
///
/// Every `'static` type with a [`Debug`] impl is a fact, identified by its type. A fact may carry a
/// value, so that a requirement can state a condition on it.
pub trait Fact: Any + Debug + Sync {}

impl<T: Any + Debug + Sync> Fact for T {}

/// A fact a declaration provides, together with its value.
pub struct ProvidedFact(Box<dyn Fact>);

impl ProvidedFact {
    fn new<T: Fact>(value: T) -> Self {
        Self(Box::new(value))
    }

    /// The type of the fact, which is what identifies it.
    pub fn type_id(&self) -> TypeId {
        (*self.0).type_id()
    }

    /// The fact itself.
    pub fn value(&self) -> &dyn Fact {
        &*self.0
    }
}

/// A fact a declaration requires, with the condition its value has to meet if there is one.
pub struct RequiredFact {
    type_id: TypeId,
    name: &'static str,
    condition: Option<DynCondition>,
}

impl RequiredFact {
    /// The type of the fact, which is what identifies it.
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// What the fact is called in an error message.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// What the value of the fact has to satisfy, if anything.
    pub fn condition(&self) -> Option<&'static str> {
        self.condition
            .as_ref()
            .map(|condition| condition.description)
    }
}

/// What a plugin provides to and requires of the composite it is part of.
pub struct Declaration {
    provided: Vec<ProvidedFact>,
    required: Vec<RequiredFact>,
}

impl Declaration {
    /// A declaration of nothing.
    pub const fn new() -> Self {
        Self {
            provided: vec![],
            required: vec![],
        }
    }

    /// The declaration `declare` makes.
    pub fn of(declare: impl FnOnce(&mut Self)) -> Self {
        let mut declaration = Self::new();
        declare(&mut declaration);
        declaration
    }

    /// Declares that `value` holds.
    ///
    /// Nothing else of the composite may provide a fact of the same type, neither another plugin
    /// nor its context.
    pub fn provide<T: Fact>(&mut self, value: T) -> &mut Self {
        self.provided.push(ProvidedFact::new(value));
        self
    }

    /// Declares that a fact of type `T` has to hold, and that its value has to meet `condition` if
    /// one is given. `name` is what the fact is called in an error message, e.g. "an elimination
    /// annotation on the matches".
    pub fn require<T: Fact>(
        &mut self,
        name: &'static str,
        condition: Option<Condition<T>>,
    ) -> &mut Self {
        self.required.push(RequiredFact {
            type_id: TypeId::of::<T>(),
            name,
            condition: condition.map(|Condition { description, holds }| DynCondition {
                description,
                holds: Box::new(move |fact| {
                    holds(fact.downcast_ref().expect("facts are keyed by their type"))
                }),
            }),
        });
        self
    }

    /// The facts this declaration provides, in the order they were declared in.
    pub fn provided(&self) -> &[ProvidedFact] {
        &self.provided
    }

    /// The facts this declaration requires, in the order they were declared in.
    pub fn required(&self) -> &[RequiredFact] {
        &self.required
    }

    /// Whether this declaration provides a fact of the type `type_id`.
    pub fn provides_type(&self, type_id: TypeId) -> bool {
        self.provided.iter().any(|fact| fact.type_id() == type_id)
    }
}

impl Default for Declaration {
    fn default() -> Self {
        Self::new()
    }
}

/// A condition a plugin requires the value of a fact of type `T` to meet.
pub struct Condition<T> {
    description: &'static str,
    holds: fn(&T) -> bool,
}

impl<T> Condition<T> {
    /// The condition that `holds`, described by `description` in an error message, e.g. "rankings
    /// of single players".
    pub const fn new(description: &'static str, holds: fn(&T) -> bool) -> Self {
        Self { description, holds }
    }
}

struct DynCondition {
    description: &'static str,
    holds: Holds,
}

/// A condition on a fact, given the fact as [`Any`].
type Holds = Box<dyn Fn(&dyn Any) -> bool + Sync>;

/// The types of the facts a plugin still to be chosen could provide.
///
/// A requirement that nothing provides yet and only such a plugin can meet is not decided, so the
/// checks treat it as met. [`Self::NOTHING`] turns that off, which is what the check of a finished
/// composite passes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OpenFacts(BTreeSet<TypeId>);

impl OpenFacts {
    /// Nothing can arrive any more, i.e. every plugin of the composite is known.
    pub const NOTHING: Self = Self(BTreeSet::new());

    /// Whether a fact of type `type_id` could still be provided.
    pub fn contains(&self, type_id: TypeId) -> bool {
        self.0.contains(&type_id)
    }
}

impl FromIterator<TypeId> for OpenFacts {
    fn from_iter<I: IntoIterator<Item = TypeId>>(types: I) -> Self {
        Self(types.into_iter().collect())
    }
}

/// Why plugins cannot be combined into one composite.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Incompatibility {
    #[error("{first} and {second} both provide {fact}")]
    FactProvidedTwice {
        fact: String,
        first: String,
        second: String,
    },
    #[error("{provider} provides {fact}, which the context provides already")]
    FactProvidedByContext { fact: String, provider: String },
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

/// What `plugin` declares with the configuration it has, together with its name as the checks
/// report it.
pub fn declaration_of<P: Plugin + Debug + ?Sized>(plugin: &P) -> (String, Declaration) {
    (
        format!("{plugin:?}"),
        Declaration::of(|declaration| plugin.total_declare(declaration)),
    )
}

/// The types of the facts required in `declarations` that none of them provides, in the order they
/// are required in and with repetitions.
pub fn unprovided(declarations: &[(String, Declaration)]) -> impl Iterator<Item = TypeId> {
    declarations
        .iter()
        .flat_map(|(_, declaration)| declaration.required())
        .map(RequiredFact::type_id)
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
pub fn order(declarations: &[(String, Declaration)]) -> Result<Vec<usize>, Incompatibility> {
    let dependencies: Vec<Vec<usize>> = declarations
        .iter()
        .enumerate()
        .map(|(index, (_, declaration))| {
            (0..declarations.len())
                .filter(|&other| {
                    other != index
                        && declaration.required().iter().any(|requirement| {
                            declarations[other].1.provides_type(requirement.type_id())
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

/// Checks that nothing in `declarations` provides a fact `context` or another one of them provides
/// already, and that every fact they require is provided and meets the condition it is required
/// with, where each declaration comes with the name of the plugin that made it.
///
/// A requirement of a fact nothing provides is not reported if its type is in `open`, since a
/// plugin still to be chosen may provide it. With [`OpenFacts::NOTHING`] everything is reported,
/// which is what the check of a finished composite passes.
///
/// # Errors
/// Returns the first incompatibility found.
pub fn check_facts(
    context: &Declaration,
    declarations: &[(String, Declaration)],
    open: &OpenFacts,
) -> Result<(), Incompatibility> {
    let mut facts = Facts::of(context);
    for (provider, declaration) in declarations {
        if let Some(conflict) = facts.conflict_with(provider, declaration) {
            return Err(conflict);
        }
        facts.add(provider, declaration);
    }
    for (required_by, declaration) in declarations {
        facts.check_requirements(required_by, declaration, open)?;
    }
    Ok(())
}

/// Checks whether the plugin `addition`, named as in [`declaration_of`], fits into a composite
/// with `context` that has the plugins with `declarations`, where the facts in `open` are the ones
/// a plugin still to be chosen could provide.
///
/// Only what `addition` declares is checked: an incompatibility among `declarations` is not
/// reported here, but by [`check_facts`].
///
/// # Errors
/// Returns the first incompatibility found.
pub fn check_addition(
    context: &Declaration,
    declarations: &[(String, Declaration)],
    addition: (&str, &Declaration),
    open: &OpenFacts,
) -> Result<(), Incompatibility> {
    let (provider, declaration) = addition;
    let mut facts = Facts::of(context);
    for (other, other_declaration) in declarations {
        facts.add(other, other_declaration);
    }
    if let Some(conflict) = facts.conflict_with(provider, declaration) {
        return Err(conflict);
    }
    // its own facts count, since a plugin may require a fact it provides itself
    facts.add(provider, declaration);
    facts.check_requirements(provider, declaration, open)
}

/// The facts of a composite by their type, each with the name of the plugin providing it, or
/// `None` for a fact of the composite's context.
struct Facts<'a>(HashMap<TypeId, (Option<&'a str>, &'a ProvidedFact)>);

impl<'a> Facts<'a> {
    /// The facts `context` provides.
    fn of(context: &'a Declaration) -> Self {
        Self(
            context
                .provided()
                .iter()
                .map(|fact| (fact.type_id(), (None, fact)))
                .collect(),
        )
    }

    /// Adds every fact `declaration` provides as provided by the plugin `provider`.
    fn add(&mut self, provider: &'a str, declaration: &'a Declaration) {
        for fact in declaration.provided() {
            self.0.insert(fact.type_id(), (Some(provider), fact));
        }
    }

    /// Why the plugin `provider` with `declaration` cannot be added to these facts, i.e. the fact
    /// of it the context or another plugin provides already.
    fn conflict_with(&self, provider: &str, declaration: &Declaration) -> Option<Incompatibility> {
        declaration.provided().iter().find_map(|fact| {
            let (first, _) = self.0.get(&fact.type_id())?;
            Some(match first {
                Some(first) => Incompatibility::FactProvidedTwice {
                    fact: format!("{:?}", fact.value()),
                    first: (*first).to_owned(),
                    second: provider.to_owned(),
                },
                None => Incompatibility::FactProvidedByContext {
                    fact: format!("{:?}", fact.value()),
                    provider: provider.to_owned(),
                },
            })
        })
    }

    /// Checks the requirements of the plugin `required_by` with `declaration` against these facts,
    /// assuming that the facts in `open` arrive.
    fn check_requirements(
        &self,
        required_by: &str,
        declaration: &Declaration,
        open: &OpenFacts,
    ) -> Result<(), Incompatibility> {
        for requirement in declaration.required() {
            // a provided fact is settled: nothing may provide it a second time, so whether its
            // value meets the condition is decided however the composite is filled up
            let Some(&(_, fact)) = self.0.get(&requirement.type_id()) else {
                if open.contains(requirement.type_id()) {
                    continue;
                }
                return Err(Incompatibility::FactNotProvided {
                    fact: requirement.name(),
                    required_by: required_by.to_owned(),
                });
            };
            if let Some(condition) = &requirement.condition
                && !(condition.holds)(fact.value())
            {
                return Err(Incompatibility::ConditionNotMet {
                    condition: condition.description,
                    required_by: required_by.to_owned(),
                    provided: format!("{:?}", fact.value()),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::iter;

    use super::*;

    #[derive(Debug)]
    struct Annotated;

    #[derive(Debug)]
    struct Derived;

    /// How many entrants a ranking stands for, a fact with a value to state a condition on.
    #[derive(Debug, PartialEq, Eq)]
    struct EntrantsPerRanking(u32);

    const SINGLE: EntrantsPerRanking = EntrantsPerRanking(1);
    const PAIRS: EntrantsPerRanking = EntrantsPerRanking(2);

    fn single_players() -> Condition<EntrantsPerRanking> {
        Condition::new("rankings of single players", |entrants| *entrants == SINGLE)
    }

    fn requires_single_players(declaration: &mut Declaration) {
        declaration.require("the entrants per ranking", Some(single_players()));
    }

    fn declared(name: &str, declare: impl FnOnce(&mut Declaration)) -> (String, Declaration) {
        (name.to_owned(), Declaration::of(declare))
    }

    /// A fact of type `T`, and nothing else, can still arrive.
    fn open<T: Fact>() -> OpenFacts {
        OpenFacts::from_iter([TypeId::of::<T>()])
    }

    /// A plugin providing [`Annotated`], used as the sub-plugin of [`Composite`].
    #[derive(Debug)]
    struct Leaf;

    impl Plugin for Leaf {
        fn id() -> &'static str {
            "leaf"
        }

        fn label() -> &'static str {
            "leaf"
        }

        fn description() -> &'static str {
            "a plugin providing an annotation"
        }

        fn static_declare(declaration: &mut Declaration) {
            declaration.provide(Annotated);
        }
    }

    /// A plugin made of a [`Leaf`], i.e. what a ranker made of other rankers is.
    #[derive(Debug)]
    struct Composite(Leaf);

    impl Plugin for Composite {
        fn id() -> &'static str {
            "composite"
        }

        fn label() -> &'static str {
            "composite"
        }

        fn description() -> &'static str {
            "a plugin made of a leaf"
        }

        fn dyn_declare(&self, declaration: &mut Declaration) {
            declaration.require::<Derived>("a derived fact", None);
        }

        fn sub_plugins(&self) -> Option<Box<dyn Iterator<Item = &dyn Plugin> + '_>> {
            Some(Box::new(iter::once(&self.0 as &dyn Plugin)))
        }
    }

    #[test]
    fn the_declaration_of_a_plugin_includes_what_its_sub_plugins_declare() {
        let (name, declaration) = declaration_of(&Composite(Leaf));

        assert_eq!(name, format!("{:?}", Composite(Leaf)));
        assert!(declaration.provides_type(TypeId::of::<Annotated>()));
        assert_eq!(
            declaration.required().iter().map(RequiredFact::name).next(),
            Some("a derived fact")
        );
    }

    #[test]
    fn plugins_are_ordered_by_any_fact_they_require() {
        let declarations = [
            declared("requiring", |declaration| {
                declaration.require::<Annotated>("an annotation", None);
            }),
            declared("independent", |_| {}),
            declared("annotating", |declaration| {
                declaration.provide(Annotated);
            }),
        ];

        assert_eq!(order(&declarations), Ok(vec![1, 2, 0]));
    }

    #[test]
    fn plugins_requiring_facts_of_each_other_cannot_be_ordered() {
        let declarations = [
            declared("annotating", |declaration| {
                declaration
                    .provide(Annotated)
                    .require::<Derived>("a derived fact", None);
            }),
            declared("deriving", |declaration| {
                declaration
                    .provide(Derived)
                    .require::<Annotated>("an annotation", None);
            }),
        ];

        assert_eq!(
            order(&declarations),
            Err(Incompatibility::CyclicRequirements {
                plugins: vec!["annotating".to_owned(), "deriving".to_owned()],
            })
        );
    }

    #[test]
    fn unprovided_lists_the_facts_no_declaration_provides() {
        let declarations = [
            declared("deriving", |declaration| {
                declaration
                    .provide(Derived)
                    .require::<Annotated>("an annotation", None);
            }),
            declared("requiring", |declaration| {
                declaration.require::<Derived>("a derived fact", None);
            }),
        ];

        assert_eq!(
            unprovided(&declarations).collect::<Vec<_>>(),
            vec![TypeId::of::<Annotated>()]
        );
    }

    #[test]
    fn a_fact_provided_by_another_plugin_meets_its_requirement() {
        let declarations = [
            declared("format", |declaration| {
                declaration.provide(Annotated);
            }),
            declared("calculator", |declaration| {
                declaration.require::<Annotated>("an annotation", None);
            }),
        ];

        assert_eq!(
            check_facts(&Declaration::new(), &declarations, &OpenFacts::NOTHING),
            Ok(())
        );
    }

    #[test]
    fn plugins_may_require_facts_of_each_other() {
        let declarations = [
            declared("format", |declaration| {
                declaration
                    .provide(Annotated)
                    .require::<EntrantsPerRanking>("the entrants per ranking", None);
            }),
            declared("ranker", |declaration| {
                declaration
                    .provide(SINGLE)
                    .require::<Annotated>("an annotation", None);
            }),
        ];

        assert_eq!(
            check_facts(&Declaration::new(), &declarations, &OpenFacts::NOTHING),
            Ok(())
        );
    }

    #[test]
    fn a_fact_nothing_provides_is_an_error() {
        let declarations = [declared("calculator", |declaration| {
            declaration.require::<Annotated>("an annotation", None);
        })];

        assert_eq!(
            check_facts(&Declaration::new(), &declarations, &OpenFacts::NOTHING),
            Err(Incompatibility::FactNotProvided {
                fact: "an annotation",
                required_by: "calculator".to_owned(),
            })
        );
    }

    #[test]
    fn a_fact_a_plugin_still_to_be_chosen_could_provide_is_no_error() {
        let declarations = [declared("calculator", |declaration| {
            declaration.require::<Annotated>("an annotation", None);
        })];

        assert_eq!(
            check_facts(&Declaration::new(), &declarations, &open::<Annotated>()),
            Ok(())
        );
    }

    #[test]
    fn a_fact_provided_by_two_plugins_is_an_error() {
        let declarations = [
            declared("format", |declaration| {
                declaration.provide(Annotated);
            }),
            declared("ranker", |declaration| {
                declaration.provide(Annotated);
            }),
        ];

        assert_eq!(
            check_facts(&Declaration::new(), &declarations, &OpenFacts::NOTHING),
            Err(Incompatibility::FactProvidedTwice {
                fact: format!("{Annotated:?}"),
                first: "format".to_owned(),
                second: "ranker".to_owned(),
            })
        );
    }

    #[test]
    fn a_condition_is_checked_against_the_fact_of_the_context() {
        let context = Declaration::of(|declaration| {
            declaration.provide(SINGLE);
        });
        let declarations = [declared("format", requires_single_players)];

        assert_eq!(
            check_facts(&context, &declarations, &OpenFacts::NOTHING),
            Ok(())
        );
    }

    #[test]
    fn a_fact_the_context_provides_cannot_be_provided_by_a_plugin() {
        let context = Declaration::of(|declaration| {
            declaration.provide(SINGLE);
        });
        let declarations = [declared("draw", |declaration| {
            declaration.provide(PAIRS);
        })];

        assert_eq!(
            check_facts(&context, &declarations, &OpenFacts::NOTHING),
            Err(Incompatibility::FactProvidedByContext {
                fact: format!("{PAIRS:?}"),
                provider: "draw".to_owned(),
            })
        );
    }

    #[test]
    fn a_condition_failing_on_a_provided_fact_is_an_error_even_if_its_type_is_open() {
        let context = Declaration::of(|declaration| {
            declaration.provide(PAIRS);
        });
        let declarations = [declared("format", requires_single_players)];

        assert_eq!(
            check_facts(&context, &declarations, &open::<EntrantsPerRanking>()),
            Err(Incompatibility::ConditionNotMet {
                condition: "rankings of single players",
                required_by: "format".to_owned(),
                provided: format!("{PAIRS:?}"),
            })
        );
    }

    #[test]
    fn an_addition_providing_a_fact_a_plugin_provides_conflicts() {
        let declarations = [declared("format", |declaration| {
            declaration.provide(Annotated);
        })];
        let addition = Declaration::of(|declaration| {
            declaration.provide(Annotated);
        });

        assert_eq!(
            check_addition(
                &Declaration::new(),
                &declarations,
                ("another format", &addition),
                &OpenFacts::NOTHING,
            ),
            Err(Incompatibility::FactProvidedTwice {
                fact: format!("{Annotated:?}"),
                first: "format".to_owned(),
                second: "another format".to_owned(),
            })
        );
    }

    #[test]
    fn an_addition_providing_a_fact_the_context_provides_conflicts() {
        let context = Declaration::of(|declaration| {
            declaration.provide(SINGLE);
        });
        let addition = Declaration::of(|declaration| {
            declaration.provide(PAIRS);
        });

        assert_eq!(
            check_addition(&context, &[], ("draw", &addition), &OpenFacts::NOTHING),
            Err(Incompatibility::FactProvidedByContext {
                fact: format!("{PAIRS:?}"),
                provider: "draw".to_owned(),
            })
        );
    }

    #[test]
    fn an_addition_is_not_blamed_for_an_incompatibility_of_the_other_plugins() {
        let declarations = [
            declared("format", |declaration| {
                declaration.provide(Annotated);
            }),
            declared("ranker", |declaration| {
                declaration.provide(Annotated);
            }),
        ];
        let addition = Declaration::of(|declaration| {
            declaration.require::<Annotated>("an annotation", None);
        });

        assert_eq!(
            check_addition(
                &Declaration::new(),
                &declarations,
                ("calculator", &addition),
                &OpenFacts::NOTHING,
            ),
            Ok(())
        );
    }

    #[test]
    fn an_addition_requiring_a_fact_nothing_provides_conflicts() {
        let addition = Declaration::of(|declaration| {
            declaration.require::<Annotated>("an annotation", None);
        });

        assert_eq!(
            check_addition(
                &Declaration::new(),
                &[],
                ("calculator", &addition),
                &OpenFacts::NOTHING
            ),
            Err(Incompatibility::FactNotProvided {
                fact: "an annotation",
                required_by: "calculator".to_owned(),
            })
        );
    }

    #[test]
    fn an_addition_may_require_a_fact_it_provides_itself() {
        let addition = Declaration::of(|declaration| {
            declaration
                .provide(SINGLE)
                .require("the entrants per ranking", Some(single_players()));
        });

        assert_eq!(
            check_addition(
                &Declaration::new(),
                &[],
                ("ranker", &addition),
                &OpenFacts::NOTHING
            ),
            Ok(())
        );
    }
}
