// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::create_downloader_retry_ac;
use super::thingiverse::model::{SearchSuccess, TvApiError};
use super::thingiverse::store::ThingStoreSlice;
use super::{
    thingiverse::store::ThingId, CreationError, Error as SuperError, Factory as IScraperFactory,
    Scraper as IScraper, TypeInfo,
};
use crate::scrapers::ok_or_return_err_stream;
use crate::{
    model::{
        hosting_provider_id::HostingProviderId,
        hosting_type::HostingType,
        hosting_unit_id::{HostingUnitId, HostingUnitIdWebById},
        project::Project,
    },
    scrapers::{
        thingiverse::model::Thing,
        thingiverse::store::{ThingMeta, ThingState, ThingStore},
    },
    settings::PartialSettings,
    structured_content::{Chunk, RawContent, SerializationFormat},
    tools::{SpdxLicenseExpression, LICENSE_UNKNOWN},
};
use async_std::{fs::File, io};
use async_stream::stream;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fs4::async_std::AsyncFileExt;
use futures::{stream::BoxStream, stream::StreamExt};
use governor::{state, Quota, RateLimiter};
use reqwest::header::HeaderMap;
use reqwest_middleware::ClientWithMiddleware;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;
use std::{borrow::Cow, fmt::Display, rc::Rc, sync::Arc};
use thiserror::Error;
use tokio::time::Duration;
use tracing::instrument;

pub mod model;
pub mod store;

pub const HOSTING_PROVIDER_ID: HostingProviderId = HostingProviderId::ThingiverseCom;

pub static SCRAPER_TYPE: LazyLock<TypeInfo> = LazyLock::new(|| TypeInfo {
    name: "thingiverse",
    description: "Scrapes projects from Thingiverse.com,
(<https://thingiverse.com/>), via their API.",
    hosting_type: HostingType::Thingiverse,
});

/// This has to be static,
/// because if we created multiple instances of [`Scraper`],
/// we would send too many requests from the same network address.
///
/// The rate limit set by Thingiverse is 300 requests per 5 minutes (== 300 seconds).
/// Instead of using this limit as it is given,
/// we resort to distributing it over the the whole time range,
/// so we get one request per second.
/// Doing it this way is probably less efficient,
/// but it makes operating the service more robust when stopping and starting it frequently,
/// which is likely the case during development phases,
/// or on user machines (vs servers).
pub static RATE_LIMITER: LazyLock<Arc<super::RL>> = LazyLock::new(|| {
    Arc::new(RateLimiter::direct(
        Quota::with_period(Duration::from_secs(1)).unwrap(),
    ))
});

// const DEFAULT_BATCH_SIZE: usize = 100;

/// same like [`PlatformBaseConfig`] but with access control.
#[derive(Deserialize, Debug)]
pub struct ThingiverseConfig {
    // hosting_provider: HostingProviderId,
    // scraper_type: String,
    /// Number of retries for a specific fetch,
    /// e.g. a batch or single project.
    retries: Option<u32>,
    /// Request timeout in milliseconds (ms)
    timeout: Option<u64>,
    // /// Batch size when fetching multiple items (e.g. projects)
    // batch_size: Option<usize>,
    access_token: String,
    things_store_root: std::path::PathBuf,
    /// Lowest thing-ID to be scraped by this scraper instance (default: 0).
    /// This is useful when employing multiple scraper instances,
    /// hosted by different machines,
    /// to segregate the total space of projects,
    /// and thus reduce the wall-clock time required to scrape everything.
    #[serde(default = "thing_id_min")]
    things_range_min: Option<ThingId>,
    /// Highest thing-ID to be scraped by this scraper instance (default: [`usize::MAX`]).
    /// This is useful when employing multiple scraper instances,
    /// hosted by different machines,
    /// to segregate the total space of projects,
    /// and thus reduce the wall-clock time required to scrape everything.
    ///
    /// NOTE: If this is larger then the latest thing-ID published on thingiverse,
    /// it will have no effect.
    #[serde(default = "thing_id_max")]
    things_range_max: Option<ThingId>,
}

#[allow(clippy::unnecessary_wraps)]
const fn thing_id_min() -> Option<ThingId> {
    Some(ThingId::MIN)
}

#[allow(clippy::unnecessary_wraps)]
const fn thing_id_max() -> Option<ThingId> {
    Some(ThingId::MAX)
}

impl super::Config for ThingiverseConfig {
    fn hosting_provider(&self) -> HostingProviderId {
        HostingProviderId::ThingiverseCom
    }
}

impl super::RetryConfig for ThingiverseConfig {
    fn retries(&self) -> Option<u32> {
        self.retries
    }

    fn timeout(&self) -> Option<u64> {
        self.timeout
    }
}

impl super::AccessControlConfig for ThingiverseConfig {
    fn access_token(&self) -> &str {
        &self.access_token
    }
}

type Config = ThingiverseConfig;

pub struct ScraperFactory;

impl IScraperFactory for ScraperFactory {
    fn info(&self) -> &'static TypeInfo {
        &SCRAPER_TYPE
    }

    fn create(
        &self,
        config_all: Arc<PartialSettings>,
        config_scraper: Value,
    ) -> Result<Box<dyn IScraper>, CreationError> {
        let config: Config = serde_json::from_value(config_scraper).map_err(|serde_err| {
            CreationError::InvalidConfig(SCRAPER_TYPE.name.to_owned(), Some(serde_err))
        })?;
        let downloader = create_downloader_retry_ac(&config);
        Ok(Box::new(Scraper {
            config_all,
            config,
            downloader,
        }))
    }
}

#[derive(Debug)]
pub struct Scraper {
    config_all: Arc<PartialSettings>,
    config: Config,
    downloader: Arc<ClientWithMiddleware>,
}

#[async_trait(?Send)]
impl IScraper for Scraper {
    fn info(&self) -> &'static TypeInfo {
        &SCRAPER_TYPE
    }

    async fn scrape(&self) -> BoxStream<'static, Result<Project, SuperError>> {
        let latest_thing_id =
            match Self::fetch_latest_thing_id(Arc::<_>::clone(&self.downloader)).await {
                Err(err) => {
                    tracing::error!("Failed to fetch latest thing-ID: {err}");
                    ok_or_return_err_stream!(Err(err));
                    ThingId::MAX
                }
                Ok(thing_id) => thing_id,
            };
        tracing::debug!("Latest thing-ID: {latest_thing_id}");

        let thing_id_range_min = std::cmp::max(
            self.config.things_range_min.unwrap_or(ThingId::MIN),
            ThingId::MIN,
        );
        let thing_id_range_max = std::cmp::min(
            latest_thing_id,
            self.config.things_range_max.unwrap_or(ThingId::MAX),
        );
        tracing::debug!("Fetching things between thing-IDs (including): {thing_id_range_min} - {thing_id_range_max}");

        tracing::info!("Preparing disc store ...");

        let store_res = ThingStore::new(
            async_std::path::PathBuf::from(self.config.things_store_root.clone()),
            thing_id_range_min,
            thing_id_range_max,
        )
        .await;

        let mut store = ok_or_return_err_stream!(store_res);

        let client = Arc::<_>::clone(&self.downloader);

        tracing::info!("Fetching {} - total ...", self.info().name);
        stream! {
            'main: loop {
                let mut store_slice = store.get_next_slice().await?;
                let mut store_slice = store_slice.write().await;
                let previously_os_things = store_slice.cloned_os(); // TODO Once we have an initial scrape, we should implement scraping these again, after this loop that scraped the yet untried
                'slice: while let Some(thing_id) = store_slice.next_id(ThingState::Untried) {
                    let res = Self::scrape_one(Arc::<_>::clone(&client), &mut store, &mut store_slice, thing_id).await;
                    let abort = if let Err(err) = &res { err.aborts() } else { false };
                    match res {
                        Err(err) => yield Err(err.into()),
                        Ok(Some(project)) => yield Ok(project),
                        Ok(None) => (),
                    }
                    if abort {
                        tracing::error!("Aborting the scraping process!");
                        break 'main;
                    }
                }
            }
        }.boxed()
    }
}

/// Thrown when a [`Scraper`] failed to scrape in general,
/// or while trying to scrape a single or a batch of projects.
#[derive(Error, Debug)]
pub enum Error {
    #[error("Some I/O problem: '{0}'")]
    IOError(#[from] std::io::Error),
    #[error(
        "Reached (and surpassed) the Thingiverse API rate-limit; Aborting the scraping process!"
    )]
    RateLimitReached,
    #[error("Network/Internet download failed: '{0}'")]
    DownloadError(#[from] reqwest::Error),
    #[error("Network/Internet download failed: '{0}'")]
    DownloadMiddlewareError(#[from] reqwest_middleware::Error),
    #[error("Failed to deserialize a fetched result to JSON: {0}")]
    DeserializeAsJsonFailed(#[from] serde_json::Error),
    #[error(
        "Failed to deserialize a fetched JSON result to our Rust model of the expected type: {0}"
    )]
    DeserializeFailed(serde_json::Error),
    #[error("Thing with ID {0} failed to fetch; API returned error: {1}")]
    HostingApiMsg(ThingId, String),
    #[error(
        "Thing with ID {0} is \"under moderation\" -> most likely flagged as copyright infringing"
    )]
    ProjectNotPublic(ThingId),
    #[error("Thing with ID {0} does not exist")]
    ProjectDoesNotExist(ThingId),
}

impl From<Error> for SuperError {
    fn from(other: Error) -> Self {
        match other {
            Error::DeserializeFailed(err) => Self::DeserializeFailed(err),
            Error::DeserializeAsJsonFailed(err) => Self::DeserializeAsJsonFailed(err),
            Error::HostingApiMsg(thing_id, msg) => {
                Self::HostingApiMsg(format!("Thing ID {thing_id} - {msg}"))
            }
            Error::ProjectNotPublic(_) => Self::ProjectNotPublic,
            Error::ProjectDoesNotExist(_) => Self::ProjectDoesNotExist,
            Error::IOError(err) => Self::IOError(err),
            Error::RateLimitReached => Self::RateLimitReached,
            Error::DownloadError(err) => Self::DownloadError(err),
            Error::DownloadMiddlewareError(err) => Self::DownloadMiddlewareError(err),
        }
    }
}

impl Error {
    pub fn as_thing_state(
        &self,
        thing_id: ThingId,
        raw_api_response: String,
    ) -> Option<(ThingState, Option<String>)> {
        Some(match self {
            Self::DeserializeFailed(err) | Self::DeserializeAsJsonFailed(err) => {
                (ThingState::FailedToParse, Some(raw_api_response))
            }
            Self::HostingApiMsg(_, _) => (ThingState::FailedToFetch, None),
            Self::ProjectNotPublic(_) => (ThingState::Banned, None),
            Self::ProjectDoesNotExist(_) => (ThingState::DoesNotExist, None),
            Self::IOError(_)
            | Self::RateLimitReached
            | Self::DownloadError(_)
            | Self::DownloadMiddlewareError(_) => {
                return None;
            }
        })
    }

    pub fn aborts(&self) -> bool {
        match self {
            Self::DeserializeFailed(_)
            | Self::DeserializeAsJsonFailed(_)
            | Self::HostingApiMsg(_, _)
            | Self::ProjectNotPublic(_)
            | Self::ProjectDoesNotExist(_) => false,
            Self::IOError(_)
            | Self::RateLimitReached
            | Self::DownloadError(_)
            | Self::DownloadMiddlewareError(_) => true,
        }
    }
}

impl Scraper {
    async fn fetch_thing_wrapper(
        client: Arc<ClientWithMiddleware>,
        thing_id: ThingId,
    ) -> Result<(ThingState, Option<String>), Error> {
        let (raw_api_response, parsed_api_response) =
            Self::fetch_thing(Arc::<_>::clone(&client), thing_id).await?;
        match parsed_api_response.into_result() {
            Ok(parsed_as_thing) => {
                let is_os = parsed_as_thing.is_open_source();
                if is_os {
                    Ok((ThingState::OpenSource, Some(raw_api_response)))
                } else {
                    Ok((ThingState::Proprietary, None))
                }
            }
            Err(err) => match err.as_thing_state(thing_id, raw_api_response) {
                Some(state) => Ok(state),
                None => Err(err),
            },
        }
    }

    async fn scrape_one(
        client: Arc<ClientWithMiddleware>,
        store: &mut ThingStore,
        store_slice: &mut ThingStoreSlice,
        thing_id: ThingId,
    ) -> Result<Option<Project>, Error> {
        // We want this time to be as close as possible to the fetch,
        // but rather before then after it.
        let fetch_time = Utc::now();
        let (state, raw_api_response) = Self::fetch_thing_wrapper(client, thing_id).await?;
        let thing_meta = ThingMeta::new(thing_id, state, fetch_time);
        store_slice
            .insert(thing_meta, raw_api_response.clone())
            .await?;
        store.set_last_scrape(fetch_time);
        Ok(if let Some(raw_api_response_val) = raw_api_response {
            let mut project = Project::new(HostingUnitId::WebById(
                HostingUnitIdWebById::from_parts(HOSTING_PROVIDER_ID, thing_id.to_string()),
            ));
            // project.crawling_meta_tree = todo!(); // TODO FIXME Set scraping meta-data!
            project.raw = Some(Chunk::from_content(
                SerializationFormat::Json,
                RawContent::String(raw_api_response_val),
            ));
            Some(project)
        } else {
            None
        })
    }
}

enum ParsedApiResponse<T: serde::de::DeserializeOwned> {
    Success(T),
    Error(ThingId, TvApiError),
    Json(serde_json::Error),
    Text(serde_json::Error),
    RateLimitReached,
}

impl<T: serde::de::DeserializeOwned> ParsedApiResponse<T> {
    fn map<NT: serde::de::DeserializeOwned>(
        self,
        success_mapper: impl FnOnce(T) -> NT,
    ) -> ParsedApiResponse<NT> {
        match self {
            Self::Success(success) => ParsedApiResponse::Success(success_mapper(success)),
            Self::Error(thing_id, err) => ParsedApiResponse::Error(thing_id, err),
            Self::Json(json) => ParsedApiResponse::Json(json),
            Self::Text(err) => ParsedApiResponse::Text(err),
            Self::RateLimitReached => ParsedApiResponse::RateLimitReached,
        }
    }

    fn into_result(self) -> Result<T, Error> {
        match self {
            Self::Success(success) => Ok(success),
            Self::Error(thing_id, api_err) => Err(Error::HostingApiMsg(thing_id, api_err.error)),
            Self::Json(err) => Err(Error::DeserializeFailed(err)),
            Self::Text(err) => Err(Error::DeserializeAsJsonFailed(err)),
            Self::RateLimitReached => Err(Error::RateLimitReached),
        }
    }
}

impl Scraper {
    // #[instrument]
    async fn fetch_as_text<P: Serialize + ?Sized + Send + Sync + std::fmt::Debug>(
        client: Arc<ClientWithMiddleware>,
        url: &str,
        params: &P,
    ) -> Result<String, Error> {
        let rate_limiter = Arc::<_>::clone(&RATE_LIMITER);
        rate_limiter.until_ready().await;
        Ok(client.get(url).query(&params).send().await?.text().await?)
    }

    fn parse_api_response_as_json(content: &str) -> Result<Value, serde_json::Error> {
        let res_json_cont = serde_json::from_str::<Value>(content);
        if let Err(err) = &res_json_cont {
            tracing::warn!("Failed to parse Thingiverse API response as JSON:\n{err}");
        }
        res_json_cont
    }

    // #[instrument]
    fn parse_api_response<T: serde::de::DeserializeOwned>(
        thing_id: ThingId,
        content: &str,
    ) -> ParsedApiResponse<T> {
        let json_val = match Self::parse_api_response_as_json(content) {
            Ok(json_val) => json_val,
            Err(serde_err) => {
                // if content == "You can only send 300 requests per 5 minutes. Please check https://www.thingiverse.com/developers/getting-started" {
                return if content.starts_with("You can only send ") {
                    ParsedApiResponse::RateLimitReached
                } else {
                    ParsedApiResponse::Text(serde_err)
                };
            }
        };

        let err_err = match serde_json::from_value::<TvApiError>(json_val.clone()) {
            Err(err) => err,
            Ok(api_error_cont) => return ParsedApiResponse::Error(thing_id, api_error_cont),
        };

        match serde_json::from_value::<T>(json_val) {
            Err(serde_err) => {
                tracing::warn!(
                    "Failed to parse Thingiverse API response (JSON), \
both as the expected type and as an error response:\n{serde_err}\n{err_err}"
                );
                ParsedApiResponse::Json(serde_err)
            }
            Ok(parsed) => ParsedApiResponse::Success(parsed),
        }
    }

    // #[instrument]
    async fn fetch_latest_thing_id(client: Arc<ClientWithMiddleware>) -> Result<ThingId, Error> {
        tracing::info!("Fetching latest thing ID ...");

        let params = [("type", "things"), ("per_page", "1"), ("sort", "newest")];
        let res_raw_text =
            Self::fetch_as_text(client, "https://api.thingiverse.com/search/", &params).await?;

        Self::parse_api_response::<SearchSuccess>(ThingId::MAX, &res_raw_text)
                .map(|success|
                    success.hits.first().expect(
                        "No hits returned when fetching latest thing from thingiverse.com - this should never happen"
                    ).id).into_result()
    }

    // #[instrument]
    async fn fetch_thing(
        client: Arc<ClientWithMiddleware>,
        thing_id: ThingId,
    ) -> Result<(String, ParsedApiResponse<Thing>), Error> {
        tracing::info!("Fetching thing {thing_id} ...");
        let params: [(&str, &str); 0] = [];
        let res_raw_text = Self::fetch_as_text(
            client,
            &format!("https://api.thingiverse.com/things/{thing_id}"),
            &params,
        )
        .await?;

        Ok(Self::parse_api_response::<Thing>(thing_id, &res_raw_text))
            .map(|thing| (res_raw_text, thing))
    }
}
