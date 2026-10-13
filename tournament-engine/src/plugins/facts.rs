//! The facts about a phase that the engine defines, which its plugins and the phase's context
//! provide and require, see [`plugin_factory::compatibility`].
//!
//! A metric is a fact too, keyed by its own type: a [`MetricsCalculator`] provides the metric it
//! calculates, and everything reading a metric requires it.
//!
//! [`MetricsCalculator`]: super::metrics_calculator::MetricsCalculator

use std::{
    any::type_name,
    fmt::{self, Debug, Formatter},
    marker::PhantomData,
    num::NonZeroU32,
};

use crate::type_map::TypeMapValue;

/// How many entrants every ranking of the phase stands for: one for rankings of single players,
/// two for a phase played between pairs.
///
/// Provided by the phase's context, which the [`Competition`] declares.
///
/// [`Competition`]: crate::entities::competition::Competition
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
pub struct MatchAnnotation<A: TypeMapValue>(PhantomData<fn() -> A>);

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
