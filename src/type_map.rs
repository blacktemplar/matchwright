use std::{
    any::{Any, TypeId},
    fmt::Debug,
};

use rustc_hash::FxHashMap;

pub trait DebugAny: Any + Debug {}
impl<T: Any + Debug> DebugAny for T {}

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
pub(crate) unsafe fn downcast_unchecked_mut<T: 'static>(value: &mut dyn Any) -> &mut T {
    debug_assert!(value.is::<T>());
    // SAFETY: as in `downcast_unchecked_ref`, and `value` is a unique borrow, so the resulting
    // `&mut T` is the only reference to the value.
    unsafe { &mut *(value as *mut dyn Any).cast::<T>() }
}

#[derive(Debug, Default)]
pub struct TypeMap(FxHashMap<TypeId, Box<dyn DebugAny>>);

impl TypeMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert<T: 'static + Debug>(&mut self, value: T) {
        self.0.insert(TypeId::of::<T>(), Box::new(value));
    }

    pub fn insert_boxed(&mut self, value: Box<dyn DebugAny>) -> Option<Box<dyn DebugAny>> {
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
