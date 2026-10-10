use std::{
    any::{Any, TypeId},
    fmt::Debug,
};

/// Something true of a phase, which the plugins of the phase and its context provide and require,
/// see [`Declaration`].
///
/// Every `'static` type with a [`Debug`] impl is a fact, identified by its type. A fact may carry a
/// value, so that a requirement can state a condition on it.
pub trait Fact: Any + Debug + Sync {}

impl<T: Any + Debug + Sync> Fact for T {}

pub struct ProvidedFact(Box<dyn Fact>);

impl ProvidedFact {
    fn new<T: Fact>(value: T) -> Self {
        Self(Box::new(value))
    }

    fn type_id(&self) -> TypeId {
        (*self.0).type_id()
    }
}

pub struct RequiredFact {
    type_id: TypeId,
    name: &'static str,
    condition: Option<DynCondition>,
}

pub struct Declaration {
    provided: Vec<ProvidedFact>,
    required: Vec<RequiredFact>,
}

impl Declaration {
    pub const fn new() -> Self {
        Self {
            provided: vec![],
            required: vec![],
        }
    }

    pub fn provide<T: Fact>(&mut self, value: T) {
        self.provided.push(ProvidedFact::new(value));
    }

    /// Whether this declaration provides a fact of the type `type_id`.
    pub(crate) fn provides_type(&self, type_id: TypeId) -> bool {
        self.provided.iter().any(|fact| fact.type_id() == type_id)
    }

    pub fn require<T: Fact>(
        &mut self,
        name: &'static str,
        condition: Option<Condition<impl Fn(&T) -> bool + Sync + 'static>>,
    ) {
        self.required.push(RequiredFact {
            type_id: TypeId::of::<T>(),
            name,
            condition: condition.map(|Condition { description, holds }| DynCondition {
                description,
                holds: Box::new(move |fact| fact.downcast_ref::<T>().is_some_and(&(holds))),
            }),
        });
    }

    pub fn provided(&self) -> &[ProvidedFact] {
        &self.provided
    }

    pub fn required(&self) -> &[RequiredFact] {
        &self.required
    }
}

pub struct Condition<H> {
    description: &'static str,
    holds: H,
}

impl<H> Condition<H> {
    pub fn new<T>(description: &'static str, holds: H) -> Self
    where
        H: Fn(&T) -> bool + Sync + 'static,
    {
        Self { description, holds }
    }
}

struct DynCondition {
    description: &'static str,
    holds: Holds,
}

/// A condition on a fact, given the fact as [`Any`].
type Holds = Box<dyn Fn(&dyn Any) -> bool + Sync>;

impl Default for Declaration {
    fn default() -> Self {
        Self::new()
    }
}
