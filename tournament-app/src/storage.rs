//! Where a [`Session`](crate::session::Session) saves its competition and loads it from.
//!
//! A competition is stored as records: one per entry of its transition log, one per person its
//! entrants are, and its [`Listing`]. The log is append-only, so a save only adds the entries
//! appended since the last one, see [`Changes`]. What a record holds is the session's business;
//! a storage keeps it as the text it is handed.

use std::{
    cell::RefCell,
    convert::Infallible,
    error::Error,
    fmt::{self, Display},
    rc::Rc,
};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tournament_engine::{EntrantId, uuid::Uuid};

/// The key a competition is stored under, independent of its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CompetitionKey(Uuid);

impl CompetitionKey {
    pub fn new(uuid: Uuid) -> Self {
        Self(uuid)
    }

    pub fn uuid(&self) -> Uuid {
        self.0
    }
}

impl Display for CompetitionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A stored competition as a list of competitions to open shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub key: CompetitionKey,
    /// The name the operator gave the competition.
    pub name: String,
}

/// What is stored of a competition, as [`Storage::read`] returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub name: String,
    /// The entries of the log, in the order they were appended.
    pub entries: Vec<String>,
    /// The people, in any order.
    pub people: Vec<String>,
}

/// What a session changed since it was last saved, to be written by [`Storage::write`].
///
/// Writing the same changes again, or changes overlapping them, stores the same: entries are
/// written at their position in the log, which never changes, and people under their entrant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changes {
    pub(crate) listing: Listing,
    pub(crate) first_entry: usize,
    pub(crate) entries: Vec<String>,
    pub(crate) people: Vec<(EntrantId, String)>,
}

impl Changes {
    /// The competition the changes are of. Writing them creates its listing if there is none yet.
    pub fn listing(&self) -> &Listing {
        &self.listing
    }

    /// The position in the log of the first of [`Self::entries`], which is at most the number of
    /// entries already stored.
    pub fn first_entry(&self) -> usize {
        self.first_entry
    }

    /// The entries to store at the positions from [`Self::first_entry`] on, replacing what is
    /// stored there.
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// The people to store, each replacing what is stored for the same entrant.
    pub fn people(&self) -> &[(EntrantId, String)] {
        &self.people
    }
}

/// Holds stored competitions.
///
/// Its methods return futures, since storage in a browser is asynchronous. A storage that is not
/// returns futures that are ready right away, which a synchronous caller can poll once.
pub trait Storage {
    type Error: Error + 'static;

    /// Every stored competition, in the order they were first written.
    ///
    /// # Errors
    /// Returns an error if the storage cannot be read.
    fn list(&self) -> impl Future<Output = Result<Vec<Listing>, Self::Error>>;

    /// What is stored of the competition `key`, or `None` if nothing is.
    ///
    /// # Errors
    /// Returns an error if the storage cannot be read.
    fn read(
        &self,
        key: CompetitionKey,
    ) -> impl Future<Output = Result<Option<Stored>, Self::Error>>;

    /// Stores `changes`, all of them or, if that fails, none.
    ///
    /// # Errors
    /// Returns an error if the changes cannot be stored, in which case what was stored before is
    /// still there.
    fn write(&self, changes: &Changes) -> impl Future<Output = Result<(), Self::Error>>;
}

/// Stores in memory, for tests and until a page stores in the browser. Its clones share what is
/// stored, so that a test can load in one session what another one saved.
#[derive(Debug, Clone, Default)]
pub struct MemoryStorage(Rc<RefCell<IndexMap<CompetitionKey, Competition>>>);

#[derive(Debug)]
struct Competition {
    name: String,
    entries: Vec<String>,
    people: IndexMap<EntrantId, String>,
}

impl Storage for MemoryStorage {
    type Error = Infallible;

    async fn list(&self) -> Result<Vec<Listing>, Infallible> {
        Ok(self
            .0
            .borrow()
            .iter()
            .map(|(key, competition)| Listing {
                key: *key,
                name: competition.name.clone(),
            })
            .collect())
    }

    async fn read(&self, key: CompetitionKey) -> Result<Option<Stored>, Infallible> {
        Ok(self.0.borrow().get(&key).map(|competition| Stored {
            name: competition.name.clone(),
            entries: competition.entries.clone(),
            people: competition.people.values().cloned().collect(),
        }))
    }

    /// # Panics
    /// Panics if the changes start after the end of the stored log, which leaves a gap.
    async fn write(&self, changes: &Changes) -> Result<(), Infallible> {
        let mut competitions = self.0.borrow_mut();
        let competition = competitions
            .entry(changes.listing.key)
            .or_insert_with(|| Competition {
                name: changes.listing.name.clone(),
                entries: Vec::new(),
                people: IndexMap::new(),
            });
        assert!(
            changes.first_entry <= competition.entries.len(),
            "the changes must continue the stored log"
        );
        // older changes written after newer ones must not cut the log short
        for (position, entry) in (changes.first_entry..).zip(&changes.entries) {
            match competition.entries.get_mut(position) {
                Some(stored) => stored.clone_from(entry),
                None => competition.entries.push(entry.clone()),
            }
        }
        competition.people.extend(changes.people.iter().cloned());
        Ok(())
    }
}
