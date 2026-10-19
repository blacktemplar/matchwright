//! The data this CLI attaches to the entities of [`tournament_engine`].
//!
//! Every type here implements [`TypeMapValue`], which makes it storable in the
//! [`TypeMap`](tournament_engine::type_map::TypeMap) of an entity and gives it an explicit
//! `typetag` name. That name is a persistence contract: renaming or moving the type must not
//! change it.

use std::fmt::{self, Display};

use serde::{Deserialize, Serialize};
use tournament_engine::type_map::TypeMapValue;

/// The name an entrant is shown and addressed by.
///
/// Stored as an annotation of an [`Entrant`](tournament_engine::entities::competition::Entrant),
/// which means it is given once, by the [`AddEntrant`] that adds that entrant.
///
/// [`AddEntrant`]: tournament_engine::transitions::add_entrant::AddEntrant
///
/// # Examples
/// ```
/// use tournament_cli::annotations::DisplayName;
/// use tournament_engine::entities::competition::{Annotated, Entrant, EntrantId};
/// use uuid::Uuid;
///
/// let entrant =
///     Entrant::new(EntrantId::new(Uuid::nil())).with_annotation(DisplayName::new("Alice"));
///
/// assert_eq!(
///     entrant.annotation::<DisplayName>(),
///     Some(&DisplayName::new("Alice"))
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DisplayName(String);

#[typetag::serde(name = "tournament_cli::display_name")]
impl TypeMapValue for DisplayName {}

impl DisplayName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for DisplayName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl From<DisplayName> for String {
    fn from(name: DisplayName) -> Self {
        name.0
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tournament_engine::entities::competition::{Annotated, Entrant, EntrantId};
    use uuid::Uuid;

    use super::*;

    fn named_entrant() -> Entrant {
        Entrant::new(EntrantId::new(Uuid::nil())).with_annotation(DisplayName::new("Alice"))
    }

    #[test]
    fn an_entrant_hands_its_name_back() {
        assert_eq!(
            named_entrant().annotation::<DisplayName>(),
            Some(&DisplayName::new("Alice"))
        );
    }

    #[test]
    fn an_entrant_without_a_name_has_none() {
        let entrant = Entrant::new(EntrantId::new(Uuid::nil()));

        assert_eq!(entrant.annotation::<DisplayName>(), None);
    }

    /// The tag is part of the serialized form of every snapshot ever taken, so a change to it is a
    /// breaking one and has to be deliberate. Pinning it here makes a rename of the type fail this
    /// test instead of silently invalidating old snapshots.
    #[test]
    fn a_name_is_serialized_under_its_explicitly_chosen_tag() {
        let value = serde_json::to_value(named_entrant()).expect("serializing must not fail");

        assert_eq!(
            value["annotations"],
            json!([{ "type": "tournament_cli::display_name", "value": "Alice" }])
        );
    }

    #[test]
    fn a_name_survives_a_round_trip_as_an_entrant_annotation() {
        let json = serde_json::to_string(&named_entrant()).expect("serializing must not fail");

        let entrant: Entrant = serde_json::from_str(&json).expect("deserializing must not fail");

        assert_eq!(
            entrant.annotation::<DisplayName>(),
            Some(&DisplayName::new("Alice"))
        );
    }

    #[test]
    fn a_name_displays_as_itself() {
        assert_eq!(DisplayName::new("Alice").to_string(), "Alice");
    }
}
