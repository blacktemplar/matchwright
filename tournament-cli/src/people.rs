//! The personal data of the entrants of a competition, such as the names this CLI shows them by.
//!
//! It is kept apart from the competition, whose transition log holds only the ids of the entrants:
//! the log has to replay as it was written, whereas personal data can be deleted.

use serde::{Deserialize, Serialize};
use tournament_engine::{
    EntrantId,
    map::{IdAble, IdMap},
};

/// Who an entrant is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    id: EntrantId,
    name: String,
}

impl Person {
    /// The person `id` is the entrant of, shown and addressed by `name`.
    pub fn new(id: EntrantId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl IdAble for Person {
    type Id = EntrantId;

    fn id(&self) -> EntrantId {
        self.id
    }
}

/// The people of a competition, by the id of the entrant each of them is.
///
/// It serializes as the list of the people in the order they were added.
///
/// # Examples
/// ```
/// use tournament_cli::people::{People, Person};
/// use tournament_engine::{EntrantId, uuid::Uuid};
///
/// let alice = EntrantId::new(Uuid::from_u128(1));
/// let mut people = People::default();
/// people.add(Person::new(alice, "Alice"));
///
/// assert_eq!(people.name(&alice), Some("Alice"));
/// assert_eq!(people.name(&EntrantId::new(Uuid::from_u128(2))), None);
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct People(IdMap<Person>);

impl People {
    /// Adds `person`, replacing whoever was the same entrant before.
    pub fn add(&mut self, person: Person) {
        self.0.insert(person);
    }

    /// The name of the entrant `id`, if this knows who it is.
    pub fn name(&self, id: &EntrantId) -> Option<&str> {
        self.0.get(id).map(Person::name)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tournament_engine::uuid::Uuid;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use super::*;

    #[test]
    fn people_serialize_as_a_list_in_the_order_they_were_added() {
        let mut people = People::default();
        people.add(Person::new(EntrantId::new(Uuid::from_u128(2)), "Bob"));
        people.add(Person::new(EntrantId::new(Uuid::from_u128(1)), "Alice"));

        let value = serde_json::to_value(&people).unwrap();

        assert_eq!(
            value,
            json!([
                { "id": "00000000-0000-0000-0000-000000000002", "name": "Bob" },
                { "id": "00000000-0000-0000-0000-000000000001", "name": "Alice" },
            ])
        );
        assert_eq!(serde_json::from_value::<People>(value).unwrap(), people);
    }

    #[test]
    fn adding_the_same_entrant_again_renames_it() {
        let id = EntrantId::new(Uuid::from_u128(1));
        let mut people = People::default();
        people.add(Person::new(id, "Alice"));

        people.add(Person::new(id, "Alicia"));

        assert_eq!(people.name(&id), Some("Alicia"));
    }
}
