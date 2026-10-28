//! Competitions stored in the browser's IndexedDB.
//!
//! A database holds three object stores, each record of them a JSON text: `competitions`, the
//! listing of each competition under its key; `entries`, the entries of the logs under
//! `[competition, position]`; and `people`, under `[competition, entrant]`. A write is one
//! transaction over all three.

use std::rc::Rc;

use futures_channel::oneshot;
use futures_util::future::try_join3;
use idb::{
    Database, KeyRange, ObjectStore, Query, TransactionMode, TransactionResult,
    builder::ObjectStoreBuilder,
};
use js_sys::Array;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tournament_app::storage::{Changes, CompetitionKey, Listing, Progress, Storage, Stored};
use wasm_bindgen::JsValue;

/// The version of the layout of the database, raised with every change of its object stores.
const VERSION: u32 = 1;

const COMPETITIONS: &str = "competitions";
const ENTRIES: &str = "entries";
const PEOPLE: &str = "people";
const STORES: [&str; 3] = [COMPETITIONS, ENTRIES, PEOPLE];

/// Stores competitions in a database of the browser's IndexedDB. Its clones share the connection
/// to it.
#[derive(Debug, Clone)]
pub struct IndexedDb(Rc<Database>);

/// Why the database could not be opened, read or written.
#[derive(Debug, Error)]
pub enum IndexedDbError {
    #[error(transparent)]
    Database(#[from] idb::Error),
    #[error("the transaction was aborted")]
    Aborted,
    #[error("a record is not what was stored")]
    Record,
    /// See [`Changes::first_entry`].
    #[error("the changes start at entry {first_entry}, but only {stored} entries are stored")]
    Gap { first_entry: usize, stored: usize },
}

/// What the listing of a competition is stored as.
#[derive(Serialize, Deserialize)]
struct ListingRecord {
    key: CompetitionKey,
    name: String,
    /// Its place in the order the competitions were stored in: one more than the number of the
    /// latest stored before it.
    number: u32,
    /// Absent in the records stored before it was.
    #[serde(default)]
    progress: Option<Progress>,
}

impl IndexedDb {
    /// Opens the database `name` of the page, creating it if there is none.
    ///
    /// # Errors
    /// Returns an error if the browser does not let the page open it, e.g. in a private window of
    /// some browsers, or it is of a newer version of the page.
    pub async fn open(name: &str) -> Result<Self, IndexedDbError> {
        let database = STORES
            .iter()
            .fold(
                Database::builder(name).version(VERSION),
                |builder, store| builder.add_object_store(ObjectStoreBuilder::new(store)),
            )
            .build()
            .await?;
        Ok(Self(Rc::new(database)))
    }
}

impl Storage for IndexedDb {
    type Error = IndexedDbError;

    async fn list(&self) -> Result<Vec<Listing>, IndexedDbError> {
        let transaction = self
            .0
            .transaction(&[COMPETITIONS], TransactionMode::ReadOnly)?;
        let records = transaction
            .object_store(COMPETITIONS)?
            .get_all(None, None)?
            .await?;
        let mut listings = records
            .into_iter()
            .map(|record| parse::<ListingRecord>(&record))
            .collect::<Result<Vec<_>, _>>()?;
        listings.sort_by_key(|listing| listing.number);
        Ok(listings
            .into_iter()
            .map(|listing| Listing {
                key: listing.key,
                name: listing.name,
                progress: listing.progress,
            })
            .collect())
    }

    async fn read(
        &self,
        key: CompetitionKey,
        first_entry: usize,
    ) -> Result<Option<Stored>, IndexedDbError> {
        let transaction = self.0.transaction(&STORES, TransactionMode::ReadOnly)?;
        let (listing, entries, people) = try_join3(
            transaction
                .object_store(COMPETITIONS)?
                .get(competition(key))?
                .into_future(),
            transaction
                .object_store(ENTRIES)?
                .get_all(Some(entries_from(key, first_entry)?), None)?
                .into_future(),
            transaction
                .object_store(PEOPLE)?
                .get_all(Some(records_of(key)?), None)?
                .into_future(),
        )
        .await?;
        let Some(listing) = listing else {
            return Ok(None);
        };
        let texts = |records: Vec<JsValue>| {
            records
                .iter()
                .map(|record| record.as_string().ok_or(IndexedDbError::Record))
                .collect::<Result<_, _>>()
        };
        Ok(Some(Stored {
            name: parse::<ListingRecord>(&listing)?.name,
            // in the order of their keys, i.e. of their positions
            entries: texts(entries)?,
            people: texts(people)?,
        }))
    }

    /// See [`Storage::write`].
    ///
    /// # Errors
    /// Returns [`IndexedDbError::Gap`] as well if the changes start after the end of the stored
    /// log, in which case nothing is stored.
    async fn write(&self, changes: &Changes) -> Result<(), IndexedDbError> {
        // The transaction stays active only while the browser hands it the result of a request, and
        // the microtasks queued then, so the writes following the reads have to be made right after
        // them. The executor of wasm-bindgen-futures continues in such a microtask; another one,
        // e.g. a GUI's, might not.
        let (sender, receiver) = oneshot::channel();
        let database = Rc::clone(&self.0);
        let changes = changes.clone();
        wasm_bindgen_futures::spawn_local(async move {
            _ = sender.send(write(&database, &changes).await);
        });
        receiver.await.unwrap_or(Err(IndexedDbError::Aborted))
    }

    async fn rename(&self, key: CompetitionKey, name: &str) -> Result<(), IndexedDbError> {
        // read and written in one transaction, as `write` is
        let (sender, receiver) = oneshot::channel();
        let database = Rc::clone(&self.0);
        let name = name.to_owned();
        wasm_bindgen_futures::spawn_local(async move {
            _ = sender.send(rename(&database, key, name).await);
        });
        receiver.await.unwrap_or(Err(IndexedDbError::Aborted))
    }

    async fn delete(&self, key: CompetitionKey) -> Result<(), IndexedDbError> {
        let transaction = self.0.transaction(&STORES, TransactionMode::ReadWrite)?;
        transaction
            .object_store(COMPETITIONS)?
            .delete(competition(key))?;
        transaction
            .object_store(ENTRIES)?
            .delete(records_of(key)?)?;
        transaction.object_store(PEOPLE)?.delete(records_of(key)?)?;
        match transaction.commit()?.await? {
            TransactionResult::Committed => Ok(()),
            TransactionResult::Aborted => Err(IndexedDbError::Aborted),
        }
    }
}

async fn rename(
    database: &Database,
    key: CompetitionKey,
    name: String,
) -> Result<(), IndexedDbError> {
    let transaction = database.transaction(&[COMPETITIONS], TransactionMode::ReadWrite)?;
    let competitions = transaction.object_store(COMPETITIONS)?;
    let Some(listing) = competitions.get(competition(key))?.await? else {
        transaction.abort()?;
        return Ok(());
    };
    let listing = ListingRecord {
        name,
        ..parse(&listing)?
    };
    put(
        &competitions,
        &serde_json::to_string(&listing).expect("a listing serializes as JSON"),
        &competition(key),
    )?;
    match transaction.commit()?.await? {
        TransactionResult::Committed => Ok(()),
        TransactionResult::Aborted => Err(IndexedDbError::Aborted),
    }
}

async fn write(database: &Database, changes: &Changes) -> Result<(), IndexedDbError> {
    let transaction = database.transaction(&STORES, TransactionMode::ReadWrite)?;
    let competitions = transaction.object_store(COMPETITIONS)?;
    let entries = transaction.object_store(ENTRIES)?;
    let people = transaction.object_store(PEOPLE)?;
    let key = changes.listing().key;
    let (listing, listings, stored) = try_join3(
        competitions.get(competition(key))?.into_future(),
        competitions.get_all(None, None)?.into_future(),
        entries.count(Some(records_of(key)?))?.into_future(),
    )
    .await?;
    // after the latest stored, whatever was deleted since
    let number = listings
        .iter()
        .map(|listing| parse::<ListingRecord>(listing).map(|listing| listing.number + 1))
        .try_fold(0, |highest, number| {
            number.map(|number| highest.max(number))
        })?;
    let stored = stored as usize;
    if changes.first_entry() > stored {
        transaction.abort()?;
        return Err(IndexedDbError::Gap {
            first_entry: changes.first_entry(),
            stored,
        });
    }
    let listing = match listing {
        None => Some(ListingRecord {
            key,
            name: changes.listing().name.clone(),
            number,
            progress: changes.listing().progress.clone(),
        }),
        Some(stored_listing) if changes.reach_end(stored) => Some(ListingRecord {
            progress: changes.listing().progress.clone(),
            ..parse(&stored_listing)?
        }),
        Some(_) => None,
    };
    if let Some(listing) = listing {
        put(
            &competitions,
            &serde_json::to_string(&listing).expect("a listing serializes as JSON"),
            &competition(key),
        )?;
    }
    for (position, entry) in (changes.first_entry()..).zip(changes.entries()) {
        put(&entries, entry, &record(key, JsValue::from(position)))?;
    }
    for (entrant, person) in changes.people() {
        // the id as the text it serializes as
        let entrant = serde_json::to_value(entrant).expect("an entrant id serializes");
        let entrant = entrant
            .as_str()
            .expect("an entrant id serializes as a text");
        put(&people, person, &record(key, JsValue::from(entrant)))?;
    }
    match transaction.commit()?.await? {
        TransactionResult::Committed => Ok(()),
        TransactionResult::Aborted => Err(IndexedDbError::Aborted),
    }
}

/// Puts `text` into `store` under `key`, without waiting for it: the transaction fails if it does.
fn put(store: &ObjectStore, text: &str, key: &JsValue) -> Result<(), idb::Error> {
    store.put(&JsValue::from_str(text), Some(key))?;
    Ok(())
}

/// The key of the competition `key` in the store of competitions.
fn competition(key: CompetitionKey) -> JsValue {
    JsValue::from(key.to_string())
}

/// The key of the record `id` of the competition `key` in the store of entries or of people.
fn record(key: CompetitionKey, id: JsValue) -> JsValue {
    Array::of2(&competition(key), &id).into()
}

/// The keys of every record of the competition `key` in the store of entries or of people: those
/// after `[key]` and before `[key, []]`, since an array is greater than any number or text.
fn records_of(key: CompetitionKey) -> Result<Query, idb::Error> {
    records_after(key, &Array::of1(&competition(key)))
}

/// The keys of the entries of the competition `key` from the position `first_entry` on.
fn entries_from(key: CompetitionKey, first_entry: usize) -> Result<Query, idb::Error> {
    records_after(key, &record(key, JsValue::from(first_entry)))
}

/// The keys of the records of the competition `key` from the key `lower` on.
fn records_after(key: CompetitionKey, lower: &JsValue) -> Result<Query, idb::Error> {
    let upper = Array::of2(&competition(key), &Array::new());
    Ok(KeyRange::bound(lower, &upper, None, None)?.into())
}

fn parse<T: for<'de> Deserialize<'de>>(record: &JsValue) -> Result<T, IndexedDbError> {
    record
        .as_string()
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or(IndexedDbError::Record)
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests;
