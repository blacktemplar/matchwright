pub mod entities;
pub mod map;
pub mod plugins;
pub mod transitions;
pub mod type_map;

/// Re-exported so that a crate registering a plugin in the
/// [catalog](plugins::catalog) does not have to depend on `inventory` itself.
pub use inventory;
