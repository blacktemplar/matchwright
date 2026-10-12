//! What plugins provide to and require of the composite they are part of, and the checks deciding
//! whether they fit together.
//!
//! A fact is anything true of a composite, a value of any `'static + Debug + Sync` type,
//! identified by its type; a fact may carry a value, so that a requirement can state a condition
//! on it. A plugin states the facts it provides and requires in a [`Declaration`], in
//! [`PluginType::declare_static`](crate::PluginType::declare_static) and [`Plugin::declare`].
//! [`check`] decides whether the plugins of a built composite fit together, and [`order`] in
//! which order they can run.

use std::{
    any::{Any, TypeId},
    collections::{BTreeSet, HashMap},
    fmt::Debug,
};

use thiserror::Error;

use crate::{__private::Fact, Plugin};

/// A fact a declaration provides, together with its value.
pub(crate) struct ProvidedFact(Box<dyn Fact>);

impl ProvidedFact {
    fn new<T: Fact>(value: T) -> Self {
        Self(Box::new(value))
    }

    /// The type of the fact, which is what identifies it.
    pub(crate) fn type_id(&self) -> TypeId {
        (*self.0).type_id()
    }

    /// The fact itself.
    pub(crate) fn value(&self) -> &dyn Fact {
        &*self.0
    }
}

/// A fact a declaration requires, with the condition its value has to meet if there is one.
pub(crate) struct RequiredFact {
    type_id: TypeId,
    name: &'static str,
    condition: Option<DynCondition>,
}

impl RequiredFact {
    /// The type of the fact, which is what identifies it.
    pub(crate) fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// What the fact is called in an error message.
    pub(crate) fn name(&self) -> &'static str {
        self.name
    }
}

/// What a plugin provides to and requires of the composite it is part of, or what the context of
/// a composite provides.
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
    pub fn provide<T: Debug + Sync + 'static>(&mut self, value: T) -> &mut Self {
        self.provided.push(ProvidedFact::new(value));
        self
    }

    /// Declares that a fact of type `T` has to hold, and that its value has to meet `condition` if
    /// one is given. `name` is what the fact is called in an error message, e.g. "an elimination
    /// annotation on the matches".
    pub fn require<T: Debug + Sync + 'static>(
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
    pub(crate) fn provided(&self) -> &[ProvidedFact] {
        &self.provided
    }

    /// The facts this declaration requires, in the order they were declared in.
    pub(crate) fn required(&self) -> &[RequiredFact] {
        &self.required
    }

    /// Whether this declaration provides a fact of the type `type_id`.
    pub(crate) fn provides_type(&self, type_id: TypeId) -> bool {
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
pub(crate) struct OpenFacts(BTreeSet<TypeId>);

impl OpenFacts {
    /// Nothing can arrive any more, i.e. every plugin of the composite is known.
    pub(crate) const NOTHING: Self = Self(BTreeSet::new());

    /// Whether a fact of type `type_id` could still be provided.
    pub(crate) fn contains(&self, type_id: TypeId) -> bool {
        self.0.contains(&type_id)
    }
}

impl FromIterator<TypeId> for OpenFacts {
    fn from_iter<I: IntoIterator<Item = TypeId>>(types: I) -> Self {
        Self(types.into_iter().collect())
    }
}

impl Extend<TypeId> for OpenFacts {
    fn extend<I: IntoIterator<Item = TypeId>>(&mut self, types: I) {
        self.0.extend(types);
    }
}

impl IntoIterator for OpenFacts {
    type Item = TypeId;
    type IntoIter = std::collections::btree_set::IntoIter<TypeId>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
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
}

/// Checks that none of `plugins` provides a fact `context` or another one of them provides
/// already, and that every fact they require is provided and meets the condition it is required
/// with. What a plugin declares includes what the plugins it holds declare, see
/// [`Configurable::plugins`](crate::schema::Configurable::plugins).
///
/// An [`Incompatibility`] names a plugin by its [`Debug`] form.
///
/// # Errors
/// Returns the first incompatibility found.
pub fn check(context: &Declaration, plugins: &[&dyn Plugin]) -> Result<(), Incompatibility> {
    check_facts(context, &declared(plugins), &OpenFacts::NOTHING)
}

/// An order in which `plugins` can run one after another, so that each runs after the plugins
/// providing the facts it requires, given as indices into `plugins`. Plugins keep their given
/// order wherever the requirements allow it, so an order that is already valid stays as it is.
///
/// A plugin may require a fact it provides itself. An [`Incompatibility`] names a plugin by its
/// [`Debug`] form.
///
/// # Errors
/// Returns [`Incompatibility::CyclicRequirements`] if plugins require facts of each other in a
/// cycle.
pub fn order(plugins: &[&dyn Plugin]) -> Result<Vec<usize>, Incompatibility> {
    order_of(&declared(plugins))
}

/// What one plugin declares, together with its name as the checks report it.
pub(crate) struct Declared {
    pub(crate) name: String,
    pub(crate) declaration: Declaration,
}

impl Declared {
    /// What `plugin` declares with the configuration it has, sub-plugins included, named by its
    /// [`Debug`] form.
    pub(crate) fn of(plugin: &dyn Plugin) -> Self {
        Self::new(
            format!("{plugin:?}"),
            Declaration::of(|declaration| plugin.declare_all(declaration)),
        )
    }

    pub(crate) fn new(name: impl Into<String>, declaration: Declaration) -> Self {
        Self {
            name: name.into(),
            declaration,
        }
    }
}

/// What each of `plugins` declares, see [`Declared::of`].
fn declared(plugins: &[&dyn Plugin]) -> Vec<Declared> {
    plugins.iter().map(|plugin| Declared::of(*plugin)).collect()
}

/// The types of the facts required in `declarations` that none of them provides, in the order they
/// are required in and with repetitions.
pub(crate) fn unprovided(declarations: &[Declared]) -> impl Iterator<Item = TypeId> {
    declarations
        .iter()
        .flat_map(|declared| declared.declaration.required())
        .map(RequiredFact::type_id)
        .filter(|&required| {
            !declarations
                .iter()
                .any(|declared| declared.declaration.provides_type(required))
        })
}

/// [`order`] of the plugins with `declarations`.
fn order_of(declarations: &[Declared]) -> Result<Vec<usize>, Incompatibility> {
    let dependencies: Vec<Vec<usize>> = declarations
        .iter()
        .enumerate()
        .map(|(index, declared)| {
            (0..declarations.len())
                .filter(|&other| {
                    other != index
                        && declared.declaration.required().iter().any(|requirement| {
                            declarations[other]
                                .declaration
                                .provides_type(requirement.type_id())
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
                    .map(|index| declarations[index].name.clone())
                    .collect(),
            });
        };
        placed[next] = true;
        order.push(next);
    }
    Ok(order)
}

/// [`check`] of the plugins with `declarations`, where a requirement of a fact nothing provides
/// is not reported if its type is in `open`, since a plugin still to be chosen may provide it.
/// With [`OpenFacts::NOTHING`] everything is reported, which is what [`check`] passes.
///
/// # Errors
/// Returns the first incompatibility found.
pub(crate) fn check_facts(
    context: &Declaration,
    declarations: &[Declared],
    open: &OpenFacts,
) -> Result<(), Incompatibility> {
    let mut facts = Facts::of(context);
    for declared in declarations {
        if let Some(conflict) = facts.conflict_with(declared) {
            return Err(conflict);
        }
        facts.add(declared);
    }
    for declared in declarations {
        facts.check_requirements(declared, open)?;
    }
    Ok(())
}

/// Checks whether the plugin with `addition` fits into a composite with `context` that has the
/// plugins with `declarations`, where the facts in `open` are the ones a plugin still to be
/// chosen could provide.
///
/// Only what `addition` declares is checked: an incompatibility among `declarations` is not
/// reported here, but by [`check_facts`].
///
/// # Errors
/// Returns the first incompatibility found.
pub(crate) fn check_addition(
    context: &Declaration,
    declarations: &[Declared],
    addition: &Declared,
    open: &OpenFacts,
) -> Result<(), Incompatibility> {
    let mut facts = Facts::of(context);
    for declared in declarations {
        facts.add(declared);
    }
    if let Some(conflict) = facts.conflict_with(addition) {
        return Err(conflict);
    }
    // its own facts count, since a plugin may require a fact it provides itself
    facts.add(addition);
    facts.check_requirements(addition, open)
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

    /// Adds every fact the plugin `provider` provides.
    fn add(&mut self, provider: &'a Declared) {
        for fact in provider.declaration.provided() {
            self.0
                .insert(fact.type_id(), (Some(provider.name.as_str()), fact));
        }
    }

    /// Why the plugin `provider` cannot be added to these facts, i.e. the fact of it the context
    /// or another plugin provides already.
    fn conflict_with(&self, provider: &Declared) -> Option<Incompatibility> {
        provider.declaration.provided().iter().find_map(|fact| {
            let (first, _) = self.0.get(&fact.type_id())?;
            Some(match first {
                Some(first) => Incompatibility::FactProvidedTwice {
                    fact: format!("{:?}", fact.value()),
                    first: (*first).to_owned(),
                    second: provider.name.clone(),
                },
                None => Incompatibility::FactProvidedByContext {
                    fact: format!("{:?}", fact.value()),
                    provider: provider.name.clone(),
                },
            })
        })
    }

    /// Checks the requirements of the plugin `required_by` against these facts, assuming that the
    /// facts in `open` arrive.
    fn check_requirements(
        &self,
        required_by: &Declared,
        open: &OpenFacts,
    ) -> Result<(), Incompatibility> {
        let Declared {
            name: required_by,
            declaration,
        } = required_by;
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
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{axis, register};

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

    fn declared(name: &str, declare: impl FnOnce(&mut Declaration)) -> Declared {
        Declared::new(name, Declaration::of(declare))
    }

    /// A fact of type `T`, and nothing else, can still arrive.
    fn open<T: Fact>() -> OpenFacts {
        OpenFacts::from_iter([TypeId::of::<T>()])
    }

    /// The axis of the plugins a [`Composite`] is made of.
    #[axis]
    trait Part: Plugin {}

    /// A plugin providing [`Annotated`], used as the sub-plugin of [`Composite`].
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "leaf",
        label = "leaf",
        description = "a plugin providing an annotation",
        declare_static = Self::declare_static
    )]
    struct Leaf;

    impl Leaf {
        fn declare_static(declaration: &mut Declaration) {
            declaration.provide(Annotated);
        }
    }

    #[register]
    impl Part for Leaf {}

    /// A plugin made of a part, i.e. what a ranker made of other rankers is.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "composite",
        label = "composite",
        description = "a plugin made of a part",
        declare = Self::declare
    )]
    struct Composite {
        #[plugin(label = "Part")]
        part: Box<dyn Part>,
    }

    impl Composite {
        fn declare(&self, declaration: &mut Declaration) {
            declaration.require::<Derived>("a derived fact", None);
        }
    }

    /// A plugin providing what [`Composite`] requires.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "deriving",
        label = "deriving",
        description = "a plugin providing a derived fact",
        declare_static = Self::declare_static
    )]
    struct Deriving;

    impl Deriving {
        fn declare_static(declaration: &mut Declaration) {
            declaration
                .provide(Derived)
                .require::<Annotated>("an annotation", None);
        }
    }

    #[register]
    impl Part for Deriving {}

    fn composite() -> Composite {
        Composite {
            part: Box::new(Leaf),
        }
    }

    #[test]
    fn the_declaration_of_a_plugin_includes_what_its_sub_plugins_declare() {
        let composite = composite();
        let Declared { name, declaration } = Declared::of(&composite);

        assert_eq!(name, format!("{composite:?}"));
        assert_eq!((&composite as &dyn Plugin).id(), "composite");
        assert!(declaration.provides_type(TypeId::of::<Annotated>()));
        assert_eq!(
            declaration.required().iter().map(RequiredFact::name).next(),
            Some("a derived fact")
        );
    }

    #[test]
    fn built_plugins_are_checked_by_what_they_and_their_sub_plugins_declare() {
        let composite = composite();

        assert_eq!(
            check(&Declaration::new(), &[&composite]),
            Err(Incompatibility::FactNotProvided {
                fact: "a derived fact",
                required_by: format!("{composite:?}"),
            })
        );
        // the annotation the deriving plugin requires comes from the leaf inside the composite
        assert_eq!(check(&Declaration::new(), &[&composite, &Deriving]), Ok(()));
    }

    #[test]
    fn built_plugins_are_ordered_by_what_they_require_of_each_other() {
        let composite = composite();

        assert_eq!(
            order(&[&composite, &Deriving]),
            Err(Incompatibility::CyclicRequirements {
                plugins: vec![format!("{composite:?}"), format!("{Deriving:?}")],
            })
        );
        assert_eq!(order(&[&Leaf, &Deriving]), Ok(vec![0, 1]));
        assert_eq!(order(&[&Deriving, &Leaf]), Ok(vec![1, 0]));
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

        assert_eq!(order_of(&declarations), Ok(vec![1, 2, 0]));
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
            order_of(&declarations),
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
        let addition = declared("another format", |declaration| {
            declaration.provide(Annotated);
        });

        assert_eq!(
            check_addition(
                &Declaration::new(),
                &declarations,
                &addition,
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
        let addition = declared("draw", |declaration| {
            declaration.provide(PAIRS);
        });

        assert_eq!(
            check_addition(&context, &[], &addition, &OpenFacts::NOTHING),
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
        let addition = declared("calculator", |declaration| {
            declaration.require::<Annotated>("an annotation", None);
        });

        assert_eq!(
            check_addition(
                &Declaration::new(),
                &declarations,
                &addition,
                &OpenFacts::NOTHING,
            ),
            Ok(())
        );
    }

    #[test]
    fn an_addition_requiring_a_fact_nothing_provides_conflicts() {
        let addition = declared("calculator", |declaration| {
            declaration.require::<Annotated>("an annotation", None);
        });

        assert_eq!(
            check_addition(&Declaration::new(), &[], &addition, &OpenFacts::NOTHING),
            Err(Incompatibility::FactNotProvided {
                fact: "an annotation",
                required_by: "calculator".to_owned(),
            })
        );
    }

    #[test]
    fn an_addition_may_require_a_fact_it_provides_itself() {
        let addition = declared("ranker", |declaration| {
            declaration
                .provide(SINGLE)
                .require("the entrants per ranking", Some(single_players()));
        });

        assert_eq!(
            check_addition(&Declaration::new(), &[], &addition, &OpenFacts::NOTHING),
            Ok(())
        );
    }
}
