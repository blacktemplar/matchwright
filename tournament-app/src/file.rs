//! A competition as a file, to keep a copy of it or to move it elsewhere: the JSON of its name, its
//! log and its people. The CLI saves the same, without the name, so each opens the other's files.
//!
//! A session is exported by [`Session::export`](crate::session::Session::export) and imported by
//! [`Session::import`](crate::session::Session::import); what is stored of a competition is
//! exported by [`Stored::export`] even if it no longer loads.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::storage::Stored;

/// What a file holds: the log `L` and the people `P`, as the CLI saves them, plus the name.
#[derive(Serialize, Deserialize)]
pub(crate) struct File<L, P> {
    /// Missing in a file of the CLI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    pub(crate) log: L,
    pub(crate) people: P,
}

impl Stored {
    /// The file of the competition stored as `self`, whether or not its log still replays: what is
    /// stored is written out as it is.
    ///
    /// # Panics
    /// Panics if the file does not serialize as JSON, which it always does.
    pub fn export(&self) -> String {
        // a record that is no JSON, which no session writes, is kept as the text it is
        let values = |records: &[String]| -> Vec<Value> {
            records
                .iter()
                .map(|record| {
                    serde_json::from_str(record).unwrap_or_else(|_| Value::String(record.clone()))
                })
                .collect()
        };
        to_json(&File {
            name: Some(self.name.clone()),
            log: values(&self.entries),
            people: values(&self.people),
        })
    }
}

/// The JSON of `file`, indented to be readable.
pub(crate) fn to_json<L: Serialize, P: Serialize>(file: &File<L, P>) -> String {
    serde_json::to_string_pretty(file).expect("a competition's file serializes as JSON")
}
