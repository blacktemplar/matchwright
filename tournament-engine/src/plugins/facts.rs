//! The facts about a phase that the engine defines, which its plugins and the phase's context
//! provide and require, see [`plugin_factory::compatibility`].

use std::{
    any::type_name,
    fmt::{self, Debug, Formatter},
    marker::PhantomData,
    num::NonZeroU32,
};

use plugin_factory::compatibility::Fact;

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

impl Fact for EntrantsPerRanking {}

// Defines a fact about a type, e.g. that a metric is calculated. Each such fact is its own named
// type rather than one generic fact over the type, since the fact is the guarantee the name states
// (every ranking holds the metric, every match carries the annotation), not the type itself.
macro_rules! type_fact {
    ($(#[$doc:meta])* $name:ident<$param:ident>, $new_doc:literal) => {
        $(#[$doc])*
        pub struct $name<$param: TypeMapValue>(PhantomData<fn() -> $param>);

        impl<$param: TypeMapValue> $name<$param> {
            #[doc = $new_doc]
            pub const fn new() -> Self {
                Self(PhantomData)
            }
        }

        impl<$param: TypeMapValue> Default for $name<$param> {
            fn default() -> Self {
                Self::new()
            }
        }

        // written out rather than derived, which would print the `PhantomData` and demand
        // `$param: Debug`
        impl<$param: TypeMapValue> Debug for $name<$param> {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "<{}>"), type_name::<$param>())
            }
        }

        impl<$param: TypeMapValue> Fact for $name<$param> {}
    };
}

type_fact!(
    /// Every ranking of the phase holds the metric `M`.
    ///
    /// Provided by the [`MetricsCalculator`] calculating `M`, and required by everything reading
    /// it, e.g. a [`RankingCriterion`] sorting by it, see [`requires_metric`].
    ///
    /// [`MetricsCalculator`]: crate::plugins::metrics_calculator::MetricsCalculator
    /// [`RankingCriterion`]: crate::plugins::ranker::RankingCriterion
    /// [`requires_metric`]: crate::plugins::ranker::requires_metric
    CalculatesMetric<M>,
    "The fact that the phase calculates `M` for every ranking."
);

type_fact!(
    /// Every match the phase plays that `A` applies to carries it as an annotation.
    ///
    /// Provided by the phase format writing the annotation, and required by everything reading it,
    /// e.g. the calculator of the [round of elimination] requires `MatchAnnotation<Eliminates>`.
    /// Which matches `A` applies to is up to the format: a double elimination annotates only the
    /// matches that can knock an entrant out with `Eliminates`.
    ///
    /// [round of elimination]: crate::plugins::metrics_calculator::round_of_elimination
    MatchAnnotation<A>,
    "The fact that the matches of the phase carry an `A` annotation."
);
