use std::{
    any::{Any, TypeId},
    fmt::Debug,
};

use rustc_hash::FxHashMap;

pub trait DebugAny: Any + Debug {}
impl<T: Any + Debug> DebugAny for T {}

#[derive(Debug, Default)]
pub struct TypeMap(FxHashMap<TypeId, Box<dyn DebugAny>>);

impl TypeMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert<T: 'static + Debug>(&mut self, value: T) {
        self.0.insert(TypeId::of::<T>(), Box::new(value));
    }

    pub fn insert_boxed(&mut self, value: Box<dyn DebugAny>) {
        let type_id = (*value).type_id();
        self.0.insert(type_id, value);
    }

    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.0.get(&TypeId::of::<T>()).map(|boxed| {
            let any: &dyn Any = boxed.as_ref();
            // SAFETY: We ensure in [`Self::insert`] that at key `TypeId::of::<T>()` an object
            // of type `T` is stored.
            unsafe { any.downcast_ref::<T>().unwrap_unchecked() }
        })
    }

    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.0.get_mut(&TypeId::of::<T>()).map(|boxed| {
            let any: &mut dyn Any = boxed.as_mut();
            // SAFETY: We ensure in [`Self::insert`] that at key `TypeId::of::<T>()` an object
            // of type `T` is stored.
            unsafe { any.downcast_mut::<T>().unwrap_unchecked() }
        })
    }
}
