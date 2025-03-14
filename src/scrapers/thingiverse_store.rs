// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{
    thingiverse_model::Thing, ACPlatformBaseConfig, CreationError, Error,
    Factory as IScraperFactory, Scraper as IScraper, TypeInfo,
};
use crate::{
    model::{
        hosting_provider_id::HostingProviderId, hosting_type::HostingType,
        hosting_unit_id::HostingUnitId, project::Project,
    },
    settings::PartialSettings,
    tools::{SpdxLicenseExpression, LICENSE_UNKNOWN, USER_AGENT_VALUE},
};
use async_std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    sync,
};
use async_stream::stream;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use core::slice;
use futures::{stream::BoxStream, stream::StreamExt};
use governor::{Quota, RateLimiter};
use reqwest::{
    header::{HeaderMap, AUTHORIZATION, USER_AGENT},
    Client,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;
use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    fmt::Display,
    sync::Arc,
};
use strum::{EnumIter, IntoEnumIterator};
use tokio::time::Duration;
use tracing::instrument;

pub const DEFAULT_SLICE_SIZE: ThingId = 1000;
pub const MIN_SLICE_SIZE: ThingId = 100;
pub const LAST_SCRAPE_FILE_NAME: &str = "last_scrape.csv";

pub type ThingId = u32;

const fn earliest() -> DateTime<Utc> {
    // TODO Maybe this would be more performant if wed make a Lazy constant and cloned it.
    DateTime::from_timestamp_nanos(0)
}

async fn ensure_dir_exists<P: AsRef<Path>>(dir: P) -> io::Result<()> {
    if !dir.as_ref().exists().await {
        fs::create_dir_all(dir.as_ref()).await?;
    }
    Ok(())
}

fn construct_file_path<P: AsRef<Path>, S: AsRef<str>>(dir: P, file_name: S, temp: bool) -> PathBuf {
    if temp {
        dir.as_ref()
            .join(format!("{file_name}.temp", file_name = file_name.as_ref()))
    } else {
        dir.as_ref().join(file_name.as_ref())
    }
}

fn last_scrape_file<P: AsRef<Path>>(dir: P, temp: bool) -> PathBuf {
    construct_file_path(dir, LAST_SCRAPE_FILE_NAME, temp)
}

async fn write_last_scrape<P: AsRef<Path> + Send>(
    temp_file: P,
    file: P,
    last_scrape: DateTime<Utc>,
) -> io::Result<()> {
    let date_str: String = last_scrape.to_rfc3339();
    fs::write(&temp_file, date_str).await?;
    fs::rename(temp_file, file).await?;
    Ok(())
}

async fn read_last_scrape<P: AsRef<Path>>(file: P) -> io::Result<Option<DateTime<Utc>>> {
    if file.as_ref().exists().await {
        let date_str = fs::read_to_string(file.as_ref()).await?;
        Ok(Some(
            DateTime::parse_from_rfc3339(&date_str)
                .map_err(|parse_err| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("Failed to parse last scrape date ({date_str}): {parse_err}"),
                    )
                })?
                .into(),
        ))
    } else {
        Ok(None)
    }
}

/// The basic state of a thing ID on the platform.
#[derive(
    Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIter,
)]
pub enum ThingState {
    /// There was some kind of error when trying to fetch the thing.
    FailedToFetch,
    /// We failed to parse the fetched thing.
    FailedToParse,
    /// The thing once existed, but is now deleted,
    /// or directly never existed on Thingiverse.
    DoesNotExist,
    /// The thing is "under moderation", meaning most likely,
    /// that some lawyer of a big company threatened to take legal actions,
    /// if the thing remains online.
    Banned,
    /// The thing exists, but has a proprietary license.
    Proprietary,
    /// The thing exists, and has an Open Source license.
    OpenSource,
    /// We have not yet tried to fetch the thing.
    Untried,
}

impl ThingState {
    #[must_use]
    pub const fn has_content(self) -> bool {
        match self {
            Self::FailedToFetch
            | Self::DoesNotExist
            | Self::Banned
            | Self::Proprietary
            | Self::Untried => false,
            Self::FailedToParse | Self::OpenSource => true,
        }
    }

    #[must_use]
    pub const fn to_str(self) -> &'static str {
        match self {
            Self::FailedToFetch => "failed_to_fetch",
            Self::FailedToParse => "failed_to_parse",
            Self::DoesNotExist => "does_not_exist",
            Self::Banned => "banned",
            Self::Proprietary => "proprietary",
            Self::OpenSource => "open_source",
            Self::Untried => "untried",
        }
    }

    const fn is_successful_fetch(self) -> bool {
        match self {
            Self::FailedToFetch
            | Self::FailedToParse
            | Self::DoesNotExist
            | Self::Banned
            | Self::Untried => false,
            Self::Proprietary | Self::OpenSource => true,
        }
    }
}

impl Display for ThingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.to_str().fmt(f)
    }
}

/// What we store for every thing ID.
#[derive(Serialize, Deserialize, Debug, Eq)]
pub struct ThingMeta {
    id: ThingId,
    state: ThingState,
    /// The first time we scraped this thing
    /// and got either a success or an error return.
    ///
    /// This will not be set if there was a network error,
    /// for example.
    #[serde(default)]
    first_scrape: Option<DateTime<Utc>>,
    /// The last time we scraped this thing
    /// and got either a success or an error return.
    ///
    /// This will not be set if there was a network error,
    /// for example.
    #[serde(default)]
    last_scrape: Option<DateTime<Utc>>,
    /// The last time we scraped this thing
    /// and got either success return.
    ///
    /// This includes both open source and proprietary results.
    #[serde(default)]
    last_successful_scrape: Option<DateTime<Utc>>,
    /// When did we last detect a change
    /// in the content returned by the API.
    ///
    /// This will coincide with a scrape-time,
    /// and not reflect the actual moment in time it changed,
    /// which is almost certainly earlier.
    #[serde(default)]
    last_change: Option<DateTime<Utc>>,
    /// How many times we tried to scrape.
    ///
    /// This includes any attempt,
    /// even if there was a network error, for example.
    #[serde(default)]
    attempted_scrapes: usize,
    /// How many times we recorded changes
    /// in the content returned by the API.
    ///
    /// This will be at most [`Self::attempted_scrapes`] - 1.
    scraped_changes: usize,
}

impl ThingMeta {
    #[must_use]
    pub const fn new(id: ThingId, state: ThingState, first_scrape: DateTime<Utc>) -> Self {
        Self {
            id,
            state,
            first_scrape: Some(first_scrape),
            last_scrape: None,
            last_successful_scrape: if state.is_successful_fetch() {
                Some(first_scrape)
            } else {
                None
            },
            last_change: None,
            attempted_scrapes: 1,
            scraped_changes: 0,
        }
    }

    pub fn normalize(&mut self) {
        let earliest = earliest();
        if let Some(first_scrape) = self.first_scrape {
            if first_scrape == earliest {
                self.first_scrape = None;
            }
        }
        if let Some(last_scrape) = self.last_scrape {
            if last_scrape == earliest {
                self.last_scrape = None;
            }
        }
        if let Some(last_change) = self.last_change {
            if last_change == earliest {
                self.last_change = None;
            }
        }
    }

    const fn new_untried(id: ThingId) -> Self {
        Self::new(id, ThingState::Untried, earliest())
    }

    #[must_use]
    pub const fn get_id(&self) -> ThingId {
        self.id
    }
}

impl PartialEq for ThingMeta {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Ord for ThingMeta {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.state.cmp(&other.state) {
            core::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        self.last_scrape.cmp(&other.last_scrape)
    }
}

impl PartialOrd for ThingMeta {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// A slice/part of a thing store, for a specific sub-range of thing IDs.
pub struct ThingStoreSlice {
    base_dir: PathBuf,
    content_dir: PathBuf,
    meta: HashMap<ThingState, VecDeque<ThingMeta>>,
    pub range_min: ThingId,
    pub range_max: ThingId,
    // things: HashMap<ThingId, Thing>,
    /// Time when the last thing was scraped, no matter its state.
    last_scrape: DateTime<Utc>,
}

impl ThingStoreSlice {
    async fn new(base_dir: PathBuf, range_min: ThingId, range_max: ThingId) -> io::Result<Self> {
        if range_max < range_min {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Programer Error: range_max ({range_max}) \
must be >= range_min ({range_min})"
                ),
            ));
        }

        let content_dir = base_dir.join("things");
        ensure_dir_exists(&content_dir).await?;
        let mut res = Self {
            base_dir,
            content_dir,
            range_min,
            range_max,
            meta: ThingState::iter()
                .map(|state| (state, VecDeque::new()))
                .collect(),
            last_scrape: earliest(),
        };
        if let Some(last_scrape) = read_last_scrape(last_scrape_file(&res.base_dir, false)).await? {
            res.last_scrape = last_scrape;
        }
        res.read().await?;

        Ok(res)
    }

    /// Returns the number of things within this slice with the given state.
    #[must_use]
    pub fn num(&self, state: ThingState) -> ThingId {
        self.meta.get(&state).unwrap().len() as ThingId
    }

    #[must_use]
    pub fn next(&self, state: ThingState) -> Option<&ThingMeta> {
        self.meta.get(&state).unwrap().front()
    }

    /// Returns the number of thing IDs covered by this slices.
    #[must_use]
    pub const fn size(&self) -> ThingId {
        self.range_max - self.range_min + 1
    }

    // pub async fn insert<S: AsRef<str>>(&mut self, thing_meta: ThingMeta, thing: Option<S>) -> Result<(), Box<dyn std::error::Error>> {
    pub async fn insert<S: AsRef<str>>(
        &mut self,
        thing_meta: ThingMeta,
        thing: Option<S>,
    ) -> io::Result<()> {
        let state = thing_meta.state;
        if matches!(state, ThingState::Untried) {
            panic!("Programer Error: State {state} should never be inserted into the store");
        }
        if let Some(thing_val) = thing {
            assert!(
                state.has_content(),
                "Programer Error: With state {:?}, \
we require no content of the thing, put it was provided",
                thing_meta.state
            );
            self.write_thing_data(thing_meta.id, thing_val).await?;
        } else if state.has_content() {
            panic!(
                "Programer Error: With state {:?}, \
we require the content of the thing, put it was not provided",
                thing_meta.state
            );
        }

        let thing_id = thing_meta.get_id();
        self.meta.get_mut(&state).unwrap().push_back(thing_meta);
        self.write(state).await?;
        if let Some(next_untried) = self.meta.get(&ThingState::Untried).unwrap().front() {
            if (next_untried.get_id() == thing_id) {
                self.meta.get_mut(&ThingState::Untried).unwrap().pop_front();
            }
        }
        Ok(())
    }

    /// Returned a cloned list of the [`ThingState::OpenSource`]` [`ThingMeta`]s.
    #[must_use]
    pub fn cloned_os(&self) -> VecDeque<ThingId> {
        self.meta
            .get(&ThingState::OpenSource)
            .unwrap()
            .iter()
            .map(|thing_meta| thing_meta.id)
            .collect()
    }

    fn meta_file_path(&self, state: ThingState, temp: bool) -> PathBuf {
        construct_file_path(&self.base_dir, format!("{state}.csv"), temp)
    }

    fn content_dir_path(&self) -> &Path {
        self.content_dir.as_path()
    }

    fn content_file_path(&self, thing_id: ThingId, temp: bool) -> PathBuf {
        construct_file_path(self.content_dir_path(), format!("{thing_id}.json"), temp)
    }

    // fn write_thing_data(&self, data: Thing) -> Result<(), Box<dyn std::error::Error>> {
    async fn write_thing_data<D: AsRef<str>>(
        &self,
        thing_id: ThingId,
        thing_json: D,
    ) -> io::Result<()> {
        let temp_file_path = self.content_file_path(thing_id, true);
        fs::write(&temp_file_path, thing_json.as_ref()).await?;
        std::fs::rename(temp_file_path, self.content_file_path(thing_id, false))?;

        Ok(())
    }

    // async fn write(&self, state: ThingState) -> Result<(), Box<dyn std::error::Error>> {
    async fn write(&self, state: ThingState) -> io::Result<()> {
        let temp_file_path = self.meta_file_path(state, true);
        {
            // let mut things_meta_writer = csv::Writer::from_path(&temp_file_path)?;
            // for thing_meta in self.meta.get(&state) {
            //     things_meta_writer.serialize(thing_meta)?;
            // }
            // things_meta_writer.flush()?;
            let mut things_meta_writer =
                csv_async::AsyncSerializer::from_writer(fs::File::create(&temp_file_path).await?);
            let values = self
                .meta
                .get(&state)
                .expect("Programer Error: All ThingStates should always be in the map");
            for thing_meta in values {
                things_meta_writer.serialize(thing_meta).await?;
            }
            things_meta_writer.flush().await?;
        }
        fs::rename(temp_file_path, self.meta_file_path(state, false)).await?;

        Ok(())
    }

    // async fn read(&mut self) -> Result<(), Box<dyn std::error::Error>> {
    async fn read(&mut self) -> io::Result<()> {
        let mut untried: BTreeSet<ThingId> = (self.range_min..=self.range_max).collect();
        for state in ThingState::iter() {
            let file_path = self.meta_file_path(state, false);
            if (file_path.exists().await) {
                let mut rdr =
                    csv_async::AsyncDeserializer::from_reader(fs::File::open(&file_path).await?);
                let mut records = rdr.deserialize::<ThingMeta>();
                while let Some(record) = records.next().await {
                    let mut thing_meta: ThingMeta = record?;
                    thing_meta.normalize();
                    untried.remove(&thing_meta.id);
                    self.meta
                        .get_mut(&thing_meta.state)
                        .unwrap()
                        .push_back(thing_meta);
                }
            }
        }
        let loaded_thing_meta_count = self
            .meta
            .values()
            .map(|v| v.len() as ThingId)
            .sum::<ThingId>();
        if loaded_thing_meta_count + untried.len() as ThingId != self.size() {
            let msg = format!(
                "Something is wrong with thing-slice {}-{} on disc: \
{} unique things in meta file, {} things (IDs) missing, \
which does not add up to the slices size: {}",
                self.range_min,
                self.range_max,
                loaded_thing_meta_count,
                untried.len(),
                self.size()
            );
            return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
        }
        let mut untried_queue = self.meta.get_mut(&ThingState::Untried).unwrap();
        for thing_id in untried {
            untried_queue.push_back(ThingMeta::new_untried(thing_id));
        }
        Ok(())
    }
}

/// A store of thing IDs in memory, with functions to read from and write to disk.
///
/// While on disc we (eventually) store the whole ranger of thing IDs on thingiverse,
/// in memory we at most cover the range of thing IDs configured for the running scraper,
/// and of these we only keep the meta info in memory, not the whole API result.
/// We may also keep only a few of the store slices in memory at any time,
/// potentially down to only the one currently being scraped.
///
/// We assume that while this scraper is running,
/// it has exclusive write access to the store oin disc.
/// If anything else writes to the store on disc while this scraper is running,
/// a corrupt state is very likely to be the result.
pub struct ThingStore {
    /// Base directory for the store
    root_dir: PathBuf,
    /// Lowest thing ID covered by the store
    range_min: ThingId,
    /// Highest thing ID covered by the store
    range_max: ThingId,
    /// Number of thing IDs covered by one store slice
    slice_size: ThingId,
    /// Store slices currently in memory, indexed by the initial thing ID they cover.
    slices: HashMap<ThingId, Arc<sync::RwLock<ThingStoreSlice>>>,
    /// Time when the last thing was scraped, no matter its state.
    last_scrape: DateTime<Utc>,
}

impl ThingStore {
    pub async fn new(
        root_dir: PathBuf,
        range_min: ThingId,
        range_max: ThingId,
    ) -> io::Result<Self> {
        if range_max < range_min {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Programer Error: range_max ({range_max}) \
must be >= range_min ({range_min})"
                ),
            ));
        }
        let slice_size = DEFAULT_SLICE_SIZE;
        if slice_size < MIN_SLICE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Programer Error: slice_size ({slice_size}) \
can not be smaller then ({MIN_SLICE_SIZE})"
                ),
            ));
        }
        //         let range = range_max - range_min + 1;
        //         if range % slice_size != 0 {
        //             return Err(io::Error::new(
        //                 io::ErrorKind::InvalidData,
        //                 format!(
        //                     "Programer Error: range (range_max - range_min + 1 => \
        // {range_max} - {range_min} + 1 = {range}) must be divisible by slice_size ({slice_size})"
        //                 ),
        //             ));
        //         }

        ensure_dir_exists(&root_dir).await?;
        let mut res = Self {
            root_dir,
            range_min,
            range_max,
            slice_size,
            slices: HashMap::new(),
            last_scrape: earliest(),
        };
        if let Some(last_scrape) = read_last_scrape(last_scrape_file(&res.root_dir, false)).await? {
            res.last_scrape = last_scrape;
        }

        Ok(res)
    }

    /// Returns the number of total thing IDs in the stores range.
    #[must_use]
    pub const fn range(&self) -> ThingId {
        self.range_max - self.range_min + 1
    }

    /// Returns the number of total/maximum slices in the stores range.
    #[must_use]
    pub const fn total_slices(&self) -> ThingId {
        self.range() / self.slice_size
    }

    // fn next(&mut self, lowest_thing_id: ThingId) -> io::Result<&mut Rc<ThingStoreSlice>> {
    //     self.slices.get_mut(&lowest_thing_id).ok_or_else(||
    //         io::Error::new(io::ErrorKind::NotFound, format!(
    //             "Slice with lowest thing ID {lowest_thing_id} not found")))
    // }

    /// The next slice to be scraped.
    /// This is either the store slice that has the oldest [`Self::last_scrape`] time,
    /// or if not all slices have been scraped yet,
    /// the un-scraped one with the lowest range.
    pub async fn get_next_slice(&mut self) -> io::Result<Arc<sync::RwLock<ThingStoreSlice>>> {
        Ok(if (self.slices.len() as ThingId) < self.total_slices() {
            // create and return a new slice
            let range_min = self.slices.len() as ThingId * self.slice_size;
            let range_max = range_min + self.slice_size;
            let base_dir = self.root_dir.join("data").join(range_min.to_string());
            ensure_dir_exists(&base_dir).await?;
            let slice = Arc::new(sync::RwLock::new(
                ThingStoreSlice::new(base_dir, range_min, range_max).await?,
            ));
            self.slices.insert(range_min, slice.clone());
            slice
        } else {
            self.slices.iter().next().unwrap().1.clone()
        })
    }

    pub const fn set_last_scrape(&mut self, time: DateTime<Utc>) {
        self.last_scrape = time;
    }
}
