//! A competition as a file, to keep a copy of it or to move it elsewhere: the JSON of its
//! [`Format`], its name, its log and its people. The CLI saves the same, without the name, so each
//! opens the other's files.
//!
//! A session is exported by [`Session::export`](crate::session::Session::export) and imported by
//! [`Session::import`](crate::session::Session::import); what is stored of a competition is
//! exported by [`Stored::export`] even if it no longer loads.

use std::fmt::{self, Display};

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;

use crate::storage::Stored;

/// The version of the format of a file, its `"format"`, raised with every change an older version
/// cannot read. A file without one is of the first.
///
/// Deserializing refuses a format newer than [`Format::CURRENT`] (or 0), so that a file of a newer
/// version is told apart from a broken one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Format(u32);

impl Format {
    /// The first format, that of a file without one.
    pub const FIRST: Self = Self(1);
    /// The format this version writes, and the newest it reads.
    pub const CURRENT: Self = Self(1);
}

impl Default for Format {
    /// [`Format::FIRST`], the format of a file without one.
    fn default() -> Self {
        Self::FIRST
    }
}

impl Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'de> Deserialize<'de> for Format {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let format = Self(u32::deserialize(deserializer)?);
        if format < Self::FIRST || format > Self::CURRENT {
            return Err(D::Error::custom(format_args!(
                "the file is of format {format}, this version reads formats {} to {}",
                Self::FIRST,
                Self::CURRENT
            )));
        }
        Ok(format)
    }
}

/// What a file holds: the log `L` and the people `P`, as the CLI saves them, plus the name.
#[derive(Serialize, Deserialize)]
pub(crate) struct File<L, P> {
    // first, so that a file of a newer format is refused for that before its log is read
    #[serde(default)]
    pub(crate) format: Format,
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
            format: Format::CURRENT,
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
