use std::{
    any::{Any, TypeId},
    fmt::Debug,
};

use serde::{
    Deserialize, Serialize,
    de::{SeqAccess, Visitor},
    ser::SerializeSeq,
};

use crate::hash_map::HashMap;

#[typetag::serde(tag = "type")]
pub trait TypeMapValue: Any + Debug {}

/// Downcasts `value` to `&T` without checking that its concrete type really is `T`.
///
/// Prefer this over `value.downcast_ref::<T>().unwrap_unchecked()` in hot paths. While
/// `unwrap_unchecked` does let LLVM drop the [`TypeId`] comparison itself, it cannot drop the
/// virtual [`Any::type_id`] call that produces the [`TypeId`], because an indirect call through the
/// `dyn Any` vtable is not known to be side effect free. Casting the fat pointer to a thin one
/// instead never touches the vtable at all.
///
/// This can be replaced by `Any::downcast_unchecked_ref` once that is stable (currently behind the
/// `downcast_unchecked` feature gate).
///
/// # Safety
/// The concrete type of `value` must be `T`.
pub(crate) unsafe fn downcast_unchecked_ref<T: 'static>(value: &dyn Any) -> &T {
    debug_assert!(value.is::<T>());
    // SAFETY: the caller guarantees that `value` has concrete type `T`, so the data pointer of the
    // fat pointer points to a valid and properly aligned `T`. Casting to a thin pointer discards
    // the vtable and keeps exactly that data pointer.
    unsafe { &*(value as *const dyn Any).cast::<T>() }
}

/// Downcasts `value` to `&mut T` without checking that its concrete type really is `T`.
///
/// See [`downcast_unchecked_ref`] for why this is preferable over
/// `downcast_mut::<T>().unwrap_unchecked()`. This can be replaced by `Any::downcast_unchecked_mut`
/// once that is stable (currently behind the `downcast_unchecked` feature gate).
///
/// # Safety
/// The concrete type of `value` must be `T`.
unsafe fn downcast_unchecked_mut<T: 'static>(value: &mut dyn Any) -> &mut T {
    debug_assert!(value.is::<T>());
    // SAFETY: as in `downcast_unchecked_ref`, and `value` is a unique borrow, so the resulting
    // `&mut T` is the only reference to the value.
    unsafe { &mut *(value as *mut dyn Any).cast::<T>() }
}

#[derive(Debug, Default)]
pub struct TypeMap(HashMap<TypeId, Box<dyn TypeMapValue>>);

impl Serialize for TypeMap {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for v in self.0.values() {
            seq.serialize_element(v)?;
        }
        seq.end()
    }
}

struct TypeMapVisitor;

impl<'de> Visitor<'de> for TypeMapVisitor {
    type Value = TypeMap;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a sequence of typetagged Box<dyn TypeMapValue>")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut map = TypeMap(HashMap::with_capacity(seq.size_hint().unwrap_or(0)));

        while let Some(value) = seq.next_element::<Box<dyn TypeMapValue>>()? {
            map.insert_boxed(value);
        }

        Ok(map)
    }
}

impl<'de> Deserialize<'de> for TypeMap {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(TypeMapVisitor)
    }
}

impl TypeMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert<T: TypeMapValue>(&mut self, value: T) {
        self.0.insert(TypeId::of::<T>(), Box::new(value));
    }

    pub fn insert_boxed(&mut self, value: Box<dyn TypeMapValue>) -> Option<Box<dyn TypeMapValue>> {
        let type_id = (*value).type_id();
        self.0.insert(type_id, value)
    }

    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.get_any::<T>().map(|any| {
            // SAFETY: We ensure in [`Self::insert`] that at key `TypeId::of::<T>()` an object
            // of type `T` is stored.
            unsafe { downcast_unchecked_ref::<T>(any) }
        })
    }

    pub fn get_any<T: 'static>(&self) -> Option<&dyn Any> {
        self.0.get(&TypeId::of::<T>()).map(|b| {
            let any: &dyn Any = b.as_ref();
            any
        })
    }

    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.0.get_mut(&TypeId::of::<T>()).map(|boxed| {
            let any: &mut dyn Any = boxed.as_mut();
            // SAFETY: We ensure in [`Self::insert`] that at key `TypeId::of::<T>()` an object
            // of type `T` is stored.
            unsafe { downcast_unchecked_mut::<T>(any) }
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::plugins::metrics_calculator::{points::Points, wins_draws_losses::WinsDrawsLosses};

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Count {
        count: u32,
    }

    #[typetag::serde]
    impl TypeMapValue for Count {}

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Label {
        text: String,
        weight: f64,
    }

    #[typetag::serde]
    impl TypeMapValue for Label {}

    /// A value whose fields are themselves compound, so that a round trip has to preserve more
    /// than a flat list of scalars.
    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
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
            json!({ "type": "Count", "count": 7 }),
            json!({ "type": "Label", "text": "buchholz", "weight": -0.5 }),
            json!({
                "type": "History",
                "entries": [{ "count": 1 }, { "count": 2 }],
                "finished": true,
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

    #[test]
    fn metric_types_of_the_engine_survive_a_round_trip() {
        let mut map = TypeMap::new();
        map.insert(Points(2.5));
        map.insert(WinsDrawsLosses {
            wins: 3,
            draws: 1,
            losses: 2,
        });

        let map = round_trip(&map);

        assert_eq!(map.get::<Points>(), Some(&Points(2.5)));
        assert_eq!(
            map.get::<WinsDrawsLosses>(),
            Some(&WinsDrawsLosses {
                wins: 3,
                draws: 1,
                losses: 2,
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
        let json = r#"[{ "type": "Count", "count": 1 }, { "type": "Count", "count": 2 }]"#;

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
        let json = r#"[{ "type": "Label", "text": "buchholz" }]"#;

        let error = serde_json::from_str::<TypeMap>(json).unwrap_err();

        assert!(
            error.to_string().contains("weight"),
            "the error must name the missing field, but was: {error}"
        );
    }
}
