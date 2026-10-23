use plugin_factory::schema::DependencyOrderedSetError;

use crate::{
    PhasePlugins,
    plugins::{
        metrics_calculator::MetricsCalculator,
        phase_format::{PhaseFormat, RoundRobin},
        ranker::SliceRanker,
    },
};

/// Builds [`PhasePlugins`] directly. Without a format, the phase is a [`RoundRobin`]; without a
/// ranking, it leaves every entrant tied; without metrics, it calculates none.
///
/// Nothing else is added: unlike a draft, the builder adds no plugin the others imply. Nor are the
/// plugins checked against each other, so a fact one of them requires may be missing; adding them
/// to a competition with [`AddPhase`](crate::AddPhase) checks that.
///
/// Requires the `test-util` feature.
///
/// # Examples
/// ```
/// use tournament_engine::{
///     PhasePluginsBuilder, Score,
///     plugins::{
///         metrics_calculator::{MorePoints, PointsCalculator, WinsDrawsLossesCalculator},
///         phase_format::SingleElimination,
///     },
/// };
///
/// // a round robin, neither ranked by anything nor calculating any metrics
/// let round_robin = PhasePluginsBuilder::new().build()?;
/// assert!(round_robin.ranking().is_empty());
///
/// let knockout = PhasePluginsBuilder::new()
///     .format(SingleElimination::new())
///     .add_ranking(MorePoints)
///     .add_metric(PointsCalculator::new(Score::new(3.0), Score::ONE))
///     .add_metric(WinsDrawsLossesCalculator)
///     .build()?;
/// // the points are scored from the wins, draws and losses, so those are calculated first
/// assert_eq!(
///     format!("{:?}", knockout.metrics()),
///     "DependencyOrderedSet([WinsDrawsLossesCalculator, PointsCalculator { points_for_win: \
///      3, points_for_draw: 1, points_for_bye: 0 }])"
/// );
/// # Ok::<(), tournament_engine::plugin_factory::schema::DependencyOrderedSetError>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct PhasePluginsBuilder {
    format: Option<Box<dyn PhaseFormat>>,
    ranking: Vec<Box<dyn SliceRanker>>,
    metrics: Vec<Box<dyn MetricsCalculator>>,
}

impl PhasePluginsBuilder {
    /// A builder with nothing given, i.e. of a round robin neither ranked by anything nor
    /// calculating any metrics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Plays the phase in `format`, replacing a format given before.
    pub fn format(mut self, format: impl PhaseFormat) -> Self {
        self.format = Some(Box::new(format));
        self
    }

    /// Ranks the phase by `criterion` after the criteria given before, i.e. `criterion` breaks the
    /// ties those leave.
    pub fn add_ranking(mut self, criterion: impl SliceRanker) -> Self {
        self.ranking.push(Box::new(criterion));
        self
    }

    /// Calculates the metric of `calculator` as well.
    pub fn add_metric(mut self, calculator: impl MetricsCalculator) -> Self {
        self.metrics.push(Box::new(calculator));
        self
    }

    /// The plugins given, with the calculators put into the order they run in, see
    /// [`PhasePluginsBuilder`].
    ///
    /// # Errors
    /// Returns an error if a calculator is given twice, or calculators require facts of each other
    /// in a cycle, see [`DependencyOrderedSet::new`].
    ///
    /// [`DependencyOrderedSet::new`]: plugin_factory::schema::DependencyOrderedSet::new
    pub fn build(self) -> Result<PhasePlugins, DependencyOrderedSetError> {
        let format = self.format.unwrap_or_else(|| Box::new(RoundRobin));
        PhasePlugins::from_parts(format, self.ranking, self.metrics)
    }
}

#[cfg(test)]
mod tests {
    use plugin_factory::{compatibility, schema::Configurable};
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use super::*;
    use crate::Competition;

    #[test]
    fn nothing_given_is_a_round_robin_that_fits_a_competition() {
        let plugins = PhasePluginsBuilder::new().build().unwrap();

        assert_eq!(format!("{:?}", plugins.format()), "RoundRobin");
        assert!(plugins.ranking().is_empty());
        assert!(plugins.metrics().is_empty());
        assert_eq!(
            compatibility::check(
                |declaration| Competition::default().declare_phase_context(declaration),
                &plugins.plugins(),
            ),
            Ok(())
        );
    }
}
