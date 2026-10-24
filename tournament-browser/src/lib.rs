//! What the GUI for running a tournament needs of the browser, apart from its page:
//!
//! - [`IndexedDb`](indexed_db::IndexedDb), the [`Storage`](tournament_app::storage::Storage) of
//!   competitions in the browser's database, one record per log entry and per person;
//! - a [`lock`] per competition, which only one tab holds at a time, through the Web Locks API;
//! - [`channel`]s that tell the other tabs what changed, and events as streams;
//! - asking the browser to [`persist`](persistence::persist) what is stored;
//! - [`download`](file::download)ing a file.
//!
//! It only works in a browser, where its tests run (in headless Chrome); natively it builds, but
//! does nothing useful.

pub mod channel;
pub mod file;
pub mod indexed_db;
pub mod lock;
pub mod persistence;
