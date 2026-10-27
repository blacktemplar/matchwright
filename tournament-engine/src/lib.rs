pub mod entities;
pub mod map;
pub mod plugins;
pub mod score;
pub mod transitions;
pub mod type_map;

#[cfg(feature = "test-util")]
pub use entities::competition::PhaseBuilder;
pub use entities::competition::{
    Annotated, Bye, ByeId, Competition, Entrant, EntrantId, Match, MatchId, MatchResult, NewMatch,
    Phase, PhaseData, PhaseId, Ranking, RankingId, ResultOfTeam, Seed, Team,
};
pub use plugins::PhasePlugins;
#[cfg(feature = "test-util")]
pub use plugins::PhasePluginsBuilder;
pub use score::Score;
pub use transitions::{
    AddEntrant, AddPhase, CompetitionStateMachine, CompetitionStateMachineExt,
    CompetitionTransition, RemoveEntrant, ReportMatchResult, SetCompetitionSeed, StartCompetition,
};
pub use type_map::{TypeMap, TypeMapValue};
// the crates the API of this one is written in, so that a user does not have to depend on them
pub use {plugin_factory, undoable_state_machine, uuid};
