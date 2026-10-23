use std::{
    any::{Any, TypeId},
    fmt::Debug,
};

use dyn_clone::DynClone;
use serde::{Deserialize, Serialize};

use crate::map::{IdAble, IdMap};

/// A value that can be stored in a [`TypeMap`], keyed by its own concrete type.
///
/// An implementor needs an explicit, namespaced `typetag` name, which is a persistence contract:
/// renaming or moving the type must not change it.
// `DynClone` is required because a transition carrying such a value has to clone it on every
// replay of the log: `Transition::apply` only takes `&self`.
#[typetag::serde(tag = "type", content = "value")]
pub trait TypeMapValue: Any + Debug + DynClone {}

dyn_clone::clone_trait_object!(TypeMapValue);

/// Downcasts `value` to `&T` without checking that its concrete type really is `T`.
///
/// Prefer this over `value.downcast_ref::<T>().unwrap_unchecked()` in hot paths.
///
/// # Safety
/// The concrete type of `value` must be `T`.
// `unwrap_unchecked` does let LLVM drop the `TypeId` comparison, but not the virtual
// `Any::type_id` call producing the `TypeId`: an indirect call through the `dyn Any` vtable is not
// known to be side effect free. Casting the fat pointer to a thin one never touches the vtable.
// Replaceable by `Any::downcast_unchecked_ref` once that is stable (feature `downcast_unchecked`).
pub(crate) unsafe fn downcast_unchecked_ref<T: 'static>(value: &dyn Any) -> &T {
    debug_assert!(value.is::<T>());
    // SAFETY: the caller guarantees that `value` has concrete type `T`, so the data pointer of the
    // fat pointer points to a valid and properly aligned `T`. Casting to a thin pointer discards
    // the vtable and keeps exactly that data pointer.
    unsafe { &*(value as *const dyn Any).cast::<T>() }
}

/// Downcasts `value` to `&mut T` without checking that its concrete type really is `T`.
///
/// Prefer this over `value.downcast_mut::<T>().unwrap_unchecked()` in hot paths, see
/// [`downcast_unchecked_ref`].
///
/// # Safety
/// The concrete type of `value` must be `T`.
// Replaceable by `Any::downcast_unchecked_mut` once that is stable (feature `downcast_unchecked`).
unsafe fn downcast_unchecked_mut<T: 'static>(value: &mut dyn Any) -> &mut T {
    debug_assert!(value.is::<T>());
    // SAFETY: as in `downcast_unchecked_ref`, and `value` is a unique borrow, so the resulting
    // `&mut T` is the only reference to the value.
    unsafe { &mut *(value as *mut dyn Any).cast::<T>() }
}

/// A map of values keyed by their own Rust type, holding at most one value per type.
///
/// A key is a [`TypeId`] rather than a name, so a lookup always gets back a value of the type it
/// asked for. This carries the metrics of a [`Ranking`](crate::entities::competition::Ranking) and
/// the consumer annotations of an [`Entrant`](crate::entities::competition::Entrant).
///
/// Serializes as a plain sequence of its values, in insertion order.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TypeMap(IdMap<Box<dyn TypeMapValue>>);

/// Keys a value by its concrete type, which is what lets [`TypeMap`] downcast without checking.
impl IdAble for Box<dyn TypeMapValue> {
    type Id = TypeId;

    #[inline]
    fn id(&self) -> TypeId {
        // `**self` rather than `self`: the type id of the boxed value, not of the box
        (**self).type_id()
    }
}

impl TypeMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts `value`, replacing the value of the same type if there is one.
    ///
    /// A replaced value keeps the position of the one it replaces, so replacing never reorders the
    /// map.
    pub fn insert<T: TypeMapValue>(&mut self, value: T) {
        self.0.insert(Box::new(value));
    }

    /// Returns this map with `value` inserted, see [`insert`](Self::insert).
    ///
    /// # Examples
    /// ```
    /// # use tournament_engine::{Score, TypeMap, plugins::metrics_calculator::Points};
    /// let map = TypeMap::new()
    ///     .with(Points(Score::new(1.0)))
    ///     .with(Points(Score::new(2.0)));
    ///
    /// assert_eq!(map.get::<Points>(), Some(&Points(Score::new(2.0))));
    /// ```
    pub fn with<T: TypeMapValue>(mut self, value: T) -> Self {
        self.insert(value);
        self
    }

    /// Like [`insert`](Self::insert) for an already boxed value, returning the value it replaced.
    pub fn insert_boxed(&mut self, value: Box<dyn TypeMapValue>) -> Option<Box<dyn TypeMapValue>> {
        self.0.insert(value)
    }

    /// The value of type `T`, or [`None`] if the map holds none.
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.0.get(&TypeId::of::<T>()).map(|boxed| {
            let any: &dyn Any = boxed.as_ref();
            // SAFETY: an `IdMap` stores every value under its `IdAble::id`, which for a
            // `Box<dyn TypeMapValue>` is the type id of its concrete type, so the value at key
            // `TypeId::of::<T>()` is a `T`.
            unsafe { downcast_unchecked_ref::<T>(any) }
        })
    }

    /// A mutable reference to the value of type `T`, or [`None`] if the map holds none.
    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.0.get_mut(&TypeId::of::<T>()).map(|boxed| {
            let any: &mut dyn Any = boxed.as_mut();
            // SAFETY: as in `get`.
            unsafe { downcast_unchecked_mut::<T>(any) }
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use super::*;
    use crate::{
        plugins::metrics_calculator::{
            buchholz::{Buchholz, BuchholzOfBuchholz, SonnebornBerger},
            points::Points,
            round_of_elimination::RoundOfElimination,
            round_of_first_loss::RoundOfFirstLoss,
            wins_draws_losses::WinsDrawsLosses,
        },
        score::Score,
    };

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct Count {
        count: u32,
    }

    #[typetag::serde]
    impl TypeMapValue for Count {}

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Label {
        text: String,
        weight: f64,
    }

    #[typetag::serde]
    impl TypeMapValue for Label {}

    /// A value whose fields are themselves compound, so that a round trip has to preserve more
    /// than a flat list of scalars.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct History {
        entries: Vec<Count>,
        finished: bool,
    }

    #[typetag::serde]
    impl TypeMapValue for History {}

    /// A map holding one value of every test type.
    fn filled_map() -> TypeMap {
        let mut map = TypeMap::new();
        map.insert(Count { count: 7 });
        map.insert(Label {
            text: "buchholz".to_owned(),
            weight: -0.5,
        });
        map.insert(History {
            entries: vec![Count { count: 1 }, Count { count: 2 }],
            finished: true,
        });
        map
    }

    fn round_trip(map: &TypeMap) -> TypeMap {
        let json = serde_json::to_string(map).expect("serializing a type map must not fail");
        serde_json::from_str(&json).expect("a serialized type map must deserialize again")
    }

    /// The serialized values of `map` as a set, since the order in which the values of a
    /// [`TypeMap`] get serialized is an implementation detail.
    fn serialized_values(map: &TypeMap) -> Vec<Value> {
        let Value::Array(mut values) = serde_json::to_value(map).unwrap() else {
            panic!("a type map must serialize to a sequence");
        };
        values.sort_by_key(ToString::to_string);
        values
    }

    #[test]
    fn an_empty_map_serializes_to_an_empty_sequence() {
        assert_eq!(serde_json::to_string(&TypeMap::new()).unwrap(), "[]");
    }

    #[test]
    fn every_value_is_serialized_as_one_tagged_element() {
        let mut expected = vec![
            json!({ "type": "Count", "value": { "count": 7 } }),
            json!({ "type": "Label", "value": { "text": "buchholz", "weight": -0.5 } }),
            json!({
                "type": "History",
                "value": {
                    "entries": [{ "count": 1 }, { "count": 2 }],
                    "finished": true,
                },
            }),
        ];
        expected.sort_by_key(ToString::to_string);

        assert_eq!(serialized_values(&filled_map()), expected);
    }

    #[test]
    fn an_empty_map_round_trips() {
        let map = round_trip(&TypeMap::new());

        assert_eq!(map.0.len(), 0);
        assert_eq!(map.get::<Count>(), None);
    }

    #[test]
    fn every_value_survives_a_round_trip() {
        let map = round_trip(&filled_map());

        assert_eq!(map.0.len(), 3);
        assert_eq!(map.get::<Count>(), Some(&Count { count: 7 }));
        assert_eq!(
            map.get::<Label>(),
            Some(&Label {
                text: "buchholz".to_owned(),
                weight: -0.5,
            })
        );
        assert_eq!(
            map.get::<History>(),
            Some(&History {
                entries: vec![Count { count: 1 }, Count { count: 2 }],
                finished: true,
            })
        );
    }

    /// The tag of a metric is part of every snapshot saved, so a change to it is a breaking one
    /// and has to be deliberate. Pinning them here makes a rename of a type fail this test instead
    /// of silently invalidating old snapshots.
    #[test]
    fn metric_types_of_the_engine_are_serialized_under_their_explicitly_chosen_tags() {
        let map = TypeMap::new()
            .with(WinsDrawsLosses::default())
            .with(Points::default())
            .with(Buchholz::default())
            .with(BuchholzOfBuchholz::default())
            .with(SonnebornBerger::default())
            .with(RoundOfFirstLoss::default())
            .with(RoundOfElimination::default());

        let value = serde_json::to_value(&map).unwrap();

        let tags: Vec<_> = value
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["type"].as_str().unwrap())
            .collect();
        assert_eq!(
            tags,
            [
                "tournament_engine::wins_draws_losses",
                "tournament_engine::points",
                "tournament_engine::buchholz",
                "tournament_engine::buchholz_of_buchholz",
                "tournament_engine::sonneborn_berger",
                "tournament_engine::round_of_first_loss",
                "tournament_engine::round_of_elimination",
            ]
        );
    }

    #[test]
    fn metric_types_of_the_engine_survive_a_round_trip() {
        let mut map = TypeMap::new();
        map.insert(Points(Score::new(2.5)));
        map.insert(WinsDrawsLosses {
            wins: 3,
            draws: 1,
            losses: 2,
            byes: 1,
        });

        let map = round_trip(&map);

        assert_eq!(map.get::<Points>(), Some(&Points(Score::new(2.5))));
        assert_eq!(
            map.get::<WinsDrawsLosses>(),
            Some(&WinsDrawsLosses {
                wins: 3,
                draws: 1,
                losses: 2,
                byes: 1,
            })
        );
    }

    #[test]
    fn a_round_trip_does_not_change_the_serialized_form() {
        let map = filled_map();
        let round_tripped = round_trip(&map);

        assert_eq!(serialized_values(&round_tripped), serialized_values(&map));
    }

    #[test]
    fn round_tripped_values_are_keyed_by_their_concrete_type() {
        let mut map = TypeMap::new();
        map.insert(Count { count: 7 });

        let mut map = round_trip(&map);

        // deserialization has to key the value by the type id of `Count`, not by the type id of
        // `Box<dyn TypeMapValue>` or anything else it got deserialized as
        *map.get_mut::<Count>().expect("the count must be found") = Count { count: 8 };
        assert_eq!(map.get::<Count>(), Some(&Count { count: 8 }));
        assert_eq!(map.get::<Label>(), None);
    }

    #[test]
    fn deserializing_the_same_type_twice_keeps_the_last_value() {
        let json = r#"[
            { "type": "Count", "value": { "count": 1 } },
            { "type": "Count", "value": { "count": 2 } }
        ]"#;

        let map: TypeMap = serde_json::from_str(json).unwrap();

        assert_eq!(map.0.len(), 1);
        assert_eq!(map.get::<Count>(), Some(&Count { count: 2 }));
    }

    #[test]
    fn deserializing_an_unknown_tag_fails() {
        let json = r#"[{ "type": "NotAKnownTypeMapValue" }]"#;

        let error = serde_json::from_str::<TypeMap>(json).unwrap_err();

        assert!(
            error.to_string().contains("NotAKnownTypeMapValue"),
            "the error must name the unknown tag, but was: {error}"
        );
    }

    #[test]
    fn deserializing_something_that_is_not_a_sequence_fails() {
        let json = r#"{ "Count": { "count": 1 } }"#;

        assert!(serde_json::from_str::<TypeMap>(json).is_err());
    }

    #[test]
    fn deserializing_a_value_with_a_missing_field_fails() {
        let json = r#"[{ "type": "Label", "value": { "text": "buchholz" } }]"#;

        let error = serde_json::from_str::<TypeMap>(json).unwrap_err();

        assert!(
            error.to_string().contains("weight"),
            "the error must name the missing field, but was: {error}"
        );
    }
}
