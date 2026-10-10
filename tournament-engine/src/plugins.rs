use std::{fmt::Debug, mem};

use dyn_clone::DynClone;

use crate::plugins::{
    compatibility::{Declaration, Incompatibility, declaration_of, order, unprovided_with_default},
    metrics_calculator::MetricsCalculator,
    phase_format::PhaseFormat,
    ranker::Ranker,
};

pub mod compatibility;
pub mod metrics_calculator;
pub mod phase_format;
pub mod ranker;

/// The supertrait of every plugin trait.
///
/// A plugin may keep state in `self` and update it in its `&mut self` methods. That state is
/// serialized together with the phase, and it must only depend on the [`PhaseData`] and the calls
/// the plugin has seen, so that replaying the transition log recomputes it.
///
/// A plugin also declares what it provides to and requires of its phase, see [`Self::declare`].
///
/// [`PhaseData`]: crate::entities::competition::PhaseData
pub trait Plugin: Debug + DynClone {
    /// Declares the facts this plugin provides to and requires of the phase it is part of, see
    /// [`Declaration`].
    ///
    /// The declaration must depend on the plugin's configuration alone, since it is made before
    /// the phase exists.
    fn declare(&self, declaration: &mut Declaration);
}

/// The plugins of one phase, which a
/// [default provider](compatibility::DefaultProvider::add_default_provider) adds to.
#[derive(Debug, Clone)]
pub struct PhasePlugins {
    pub(crate) phase_format: Box<dyn PhaseFormat>,
    pub(crate) ranker: Box<dyn Ranker>,
    pub(crate) metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
}

impl PhasePlugins {
    pub(crate) fn new(
        phase_format: Box<dyn PhaseFormat>,
        ranker: Box<dyn Ranker>,
        metrics_calculators: Vec<Box<dyn MetricsCalculator>>,
    ) -> Self {
        Self {
            phase_format,
            ranker,
            metrics_calculators,
        }
    }

    pub fn phase_format(&self) -> &dyn PhaseFormat {
        &*self.phase_format
    }

    pub fn ranker(&self) -> &dyn Ranker {
        &*self.ranker
    }

    /// The metrics calculators, in the order they run in.
    pub fn metrics_calculators(&self) -> &[Box<dyn MetricsCalculator>] {
        &self.metrics_calculators
    }

    /// Adds `metrics_calculator` after the metrics calculators already there.
    pub fn add_metrics_calculator(&mut self, metrics_calculator: Box<dyn MetricsCalculator>) {
        self.metrics_calculators.push(metrics_calculator);
    }

    /// The declarations of all plugins, each together with the name of its plugin.
    pub(crate) fn declarations(&self) -> Vec<(String, Declaration)> {
        [
            declaration_of(&*self.phase_format),
            declaration_of(&*self.ranker),
        ]
        .into_iter()
        .chain(self.declarations_of_metrics_calculators())
        .collect()
    }

    fn declarations_of_metrics_calculators(&self) -> Vec<(String, Declaration)> {
        self.metrics_calculators
            .iter()
            .map(|metrics_calculator| declaration_of(&**metrics_calculator))
            .collect()
    }

    /// Adds the default provider of every required fact nothing provides, including those the
    /// added providers require, and orders the metrics calculators so that each runs after the
    /// ones providing the facts it requires, keeping their order otherwise.
    ///
    /// # Errors
    /// Returns an error if metrics calculators require facts of each other in a cycle.
    pub(crate) fn resolve(&mut self) -> Result<(), Incompatibility> {
        // every fact gets one try, so that a fact without default provider, or with one that does
        // not provide it, does not stop the loop from ending
        let mut tried = Vec::new();
        loop {
            let declarations = self.declarations();
            let Some((fact, add_default_provider)) =
                unprovided_with_default(&declarations).find(|(fact, _)| !tried.contains(fact))
            else {
                break;
            };
            tried.push(fact);
            add_default_provider(self);
        }

        let order = order(&self.declarations_of_metrics_calculators())?;
        let mut metrics_calculators: Vec<_> = mem::take(&mut self.metrics_calculators)
            .into_iter()
            .map(Some)
            .collect();
        self.metrics_calculators = order
            .into_iter()
            .map(|index| {
                metrics_calculators[index]
                    .take()
                    .expect("an order holds every index once")
            })
            .collect();
        Ok(())
    }

    /// Checks that the metrics calculators are ordered as [`Self::resolve`] orders them.
    ///
    /// # Errors
    /// Returns an error if they are not, or if they require facts of each other in a cycle.
    pub(crate) fn check_order(&self) -> Result<(), Incompatibility> {
        let order = order(&self.declarations_of_metrics_calculators())?;
        match order
            .into_iter()
            .enumerate()
            .find(|(position, index)| position != index)
        {
            Some((position, _)) => Err(Incompatibility::OutOfOrder {
                plugin: format!("{:?}", self.metrics_calculators[position]),
            }),
            None => Ok(()),
        }
    }
}

/// What the plugins of this crate declare, i.e. the facts they provide to and require of their
/// phase. The mechanism itself is tested in [`compatibility`].
#[cfg(test)]
mod tests {
    use std::{
        any::{Any, TypeId, type_name},
        num::NonZeroU32,
    };

    use super::*;
    use crate::{
        entities::competition::Competition,
        plugins::{
            compatibility::{EntrantsPerRanking, MatchAnnotation, check_facts, declaration_of},
            metrics_calculator::{
                buchholz::{
                    Buchholz, BuchholzCalculator, BuchholzOfBuchholz, BuchholzOfBuchholzCalculator,
                },
                matches::{Matches, MatchesCalculator},
                points::{MorePoints, Points, PointsCalculator},
                round_of_elimination::{
                    Eliminates, LaterElimination, RoundOfEliminationCalculator,
                },
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
            ranker::{HierarchicalRanker, LexicographicalCriteria},
        },
    };

    /// The scoring system of chess: a win is worth a point, a draw half of one.
    const ONE_HALF: PointsCalculator = PointsCalculator::new(1.0, 0.5);
    /// The classic Sonneborn-Berger weights: a beaten opponent counts fully, an opponent played to
    /// a draw counts half.
    const FULL_HALF: SonnebornBergerCalculator = SonnebornBergerCalculator::new(1.0, 0.5);
    /// Two entrants per ranking, i.e. the rankings a draw pairing the entrants up leaves behind.
    const PAIRS: EntrantsPerRanking = EntrantsPerRanking(NonZeroU32::new(2).unwrap());

    /// A ranker sorting by `criteria`, which is how every phase of the CLI is ranked.
    fn ranked_by(criteria: LexicographicalCriteria) -> Box<dyn Ranker> {
        Box::new(HierarchicalRanker::new(vec![Box::new(criteria)]))
    }

    /// A ranker leaving every entrant tied, for a phase whose ranking is not what is tested.
    fn unranked() -> Box<dyn Ranker> {
        ranked_by(LexicographicalCriteria::new())
    }

    /// Whether the plugins of a phase played in `format`, ranked by `ranker` and calculating the
    /// metrics of `calculators` fit together in a competition, i.e. what `AddPhase` checks.
    fn check(
        format: Box<dyn PhaseFormat>,
        ranker: Box<dyn Ranker>,
        calculators: Vec<Box<dyn MetricsCalculator>>,
    ) -> Result<(), Incompatibility> {
        let competition = Competition::default();
        let context = Declaration::of(|declaration| competition.declare_phase_context(declaration));
        check_in(context, format, ranker, calculators)
    }

    /// Like [`check`], for a phase whose context provides `context` rather than a competition.
    fn check_in(
        context: Declaration,
        format: Box<dyn PhaseFormat>,
        ranker: Box<dyn Ranker>,
        calculators: Vec<Box<dyn MetricsCalculator>>,
    ) -> Result<(), Incompatibility> {
        let plugins = PhasePlugins::new(format, ranker, calculators);
        check_facts(context, &plugins.declarations())
    }

    /// The metrics the calculators of `plugins` calculate, in the order they run in.
    fn metrics(plugins: &PhasePlugins) -> Vec<TypeId> {
        plugins
            .metrics_calculators()
            .iter()
            .map(|calculator| (&*calculator.default_metric_value() as &dyn Any).type_id())
            .collect()
    }

    #[test]
    fn a_calculator_runs_after_the_calculators_of_the_metrics_it_reads() {
        // listed so that every calculator reading a metric comes before the one calculating it
        let mut plugins = PhasePlugins::new(
            Box::new(RoundRobin),
            unranked(),
            vec![
                Box::new(BuchholzOfBuchholzCalculator),
                Box::new(FULL_HALF),
                Box::new(BuchholzCalculator),
                Box::new(ONE_HALF),
                Box::new(MatchesCalculator),
                Box::new(WinsDrawsLossesCalculator),
            ],
        );

        plugins.resolve().unwrap();

        assert_eq!(
            metrics(&plugins),
            [
                TypeId::of::<Matches>(),
                TypeId::of::<WinsDrawsLosses>(),
                TypeId::of::<Points>(),
                // the Sonneborn-Berger keeps its place before the Buchholz, as neither reads the
                // other
                TypeId::of::<SonnebornBerger>(),
                TypeId::of::<Buchholz>(),
                TypeId::of::<BuchholzOfBuchholz>(),
            ]
        );
    }

    #[test]
    fn a_round_robin_ranked_by_the_sonneborn_berger_fits_with_every_metric_it_reads() {
        let ranker = ranked_by(
            LexicographicalCriteria::new()
                .add_criterion(MorePoints)
                .add_criterion(MoreSonnebornBerger),
        );

        assert_eq!(
            check(
                Box::new(RoundRobin),
                ranker,
                vec![
                    Box::new(WinsDrawsLossesCalculator),
                    Box::new(ONE_HALF),
                    Box::new(MatchesCalculator),
                    Box::new(FULL_HALF),
                ],
            ),
            Ok(())
        );
    }

    #[test]
    fn a_metric_a_calculator_reads_has_to_be_calculated_as_well() {
        // the Sonneborn-Berger sums the points of the opponents a ranking met, which it reads from
        // the matches of that ranking
        assert_eq!(
            check(
                Box::new(RoundRobin),
                unranked(),
                vec![
                    Box::new(WinsDrawsLossesCalculator),
                    Box::new(ONE_HALF),
                    Box::new(FULL_HALF),
                ],
            ),
            Err(Incompatibility::FactNotProvided {
                fact: type_name::<Matches>(),
                required_by: format!("{FULL_HALF:?}"),
            })
        );
    }

    #[test]
    fn ranking_by_a_metric_nothing_calculates_is_an_error() {
        let ranker = ranked_by(LexicographicalCriteria::new().add_criterion(MorePoints));

        assert_eq!(
            check(
                Box::new(RoundRobin),
                ranker.clone(),
                vec![Box::new(WinsDrawsLossesCalculator)],
            ),
            Err(Incompatibility::FactNotProvided {
                fact: type_name::<Points>(),
                required_by: format!("{ranker:?}"),
            })
        );
    }

    /// A criterion states the metric it sorts by through its [`RankingOrder`], whichever of the
    /// two methods it uses, so the ranker requires it either way.
    ///
    /// [`RankingOrder`]: ranker::RankingOrder
    #[test]
    fn a_criterion_bringing_its_own_order_requires_its_metric_too() {
        let ranker = ranked_by(LexicographicalCriteria::new().add_criterion(FewerLosses));

        assert_eq!(
            check(Box::new(RoundRobin), ranker.clone(), Vec::new()),
            Err(Incompatibility::FactNotProvided {
                fact: type_name::<WinsDrawsLosses>(),
                required_by: format!("{ranker:?}"),
            })
        );
    }

    #[test]
    fn the_round_of_elimination_needs_a_format_that_knocks_entrants_out() {
        let ranker = ranked_by(LexicographicalCriteria::new().add_criterion(LaterElimination));
        let calculators: Vec<Box<dyn MetricsCalculator>> =
            vec![Box::new(RoundOfEliminationCalculator)];

        assert_eq!(
            check(
                Box::new(DoubleElimination::new()),
                ranker.clone(),
                calculators.clone(),
            ),
            Ok(())
        );
        assert_eq!(
            check(Box::new(RoundRobin), ranker, calculators),
            Err(Incompatibility::FactNotProvided {
                fact: type_name::<MatchAnnotation<Eliminates>>(),
                required_by: format!("{RoundOfEliminationCalculator:?}"),
            })
        );
    }

    /// The annotation naming the stage of a match is declared like every other one, so that a
    /// plugin reading it — a display of the bracket, say — can require it.
    #[test]
    fn a_bracket_format_provides_the_annotations_it_writes() {
        let (_, knockout) = declaration_of(&SingleElimination::new());
        let (_, double_elimination) = declaration_of(&DoubleElimination::new());

        assert!(knockout.provides_type(TypeId::of::<MatchAnnotation<KnockoutRound>>()));
        assert!(
            double_elimination
                .provides_type(TypeId::of::<MatchAnnotation<DoubleEliminationRound>>())
        );
        assert!(double_elimination.provides_type(TypeId::of::<MatchAnnotation<Eliminates>>()));
    }

    #[test]
    fn a_monster_dyp_fits_a_competition_whose_rankings_are_single_entrants() {
        assert_eq!(
            check(Box::new(MonsterDyp::new(3)), unranked(), Vec::new()),
            Ok(())
        );
    }

    #[test]
    fn a_monster_dyp_cannot_follow_a_draw_that_paired_the_entrants_up() {
        let pairs = Declaration::of(|declaration| {
            declaration.provides(PAIRS);
        });
        let format = MonsterDyp::new(3);

        assert_eq!(
            check_in(pairs, Box::new(format.clone()), unranked(), Vec::new()),
            Err(Incompatibility::ConditionNotMet {
                condition: "rankings of single players",
                required_by: format!("{format:?}"),
                provided: format!("{PAIRS:?}"),
            })
        );
    }
}
