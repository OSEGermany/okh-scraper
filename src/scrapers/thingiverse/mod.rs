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
        let downloader = create_downloader_retry_ac(config_all.as_ref(), &config);
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
        tracing::debug!("Scraping things between thing-IDs (including): {thing_id_range_min} - {thing_id_range_max}");

        tracing::info!("Preparing disc store ...");

        let store_res = ThingStore::new(
            async_std::path::PathBuf::from(self.config.things_store_root.clone()),
            thing_id_range_min,
            thing_id_range_max,
        )
        .await;

        let mut store = ok_or_return_err_stream!(store_res);

        let client = Arc::<_>::clone(&self.downloader);

        tracing::info!("Scraping {} ...", self.info().name);
        tracing::debug!("Looking for store slice with untired things ...");
        stream! {
            'main: loop {
                let mut store_slice = store.get_next_slice().await?;
                let mut store_slice = store_slice.write().await;
                tracing::debug!("Looking for untired things in store slice {} ...", store_slice.range_min);
                let previously_os_things = store_slice.cloned_os(); // TODO Once we have an initial scrape, we should implement scraping these again, after this loop that scraped the yet untried
                'slice: while let Some(thing_id) = store_slice.next_id(ThingState::Untried) {
                    let res = Self::scrape_one(Arc::<_>::clone(&client), &mut store, &mut store_slice, thing_id).await;
                    let abort = if let Err(err) = &res { err.aborts() } else { false };
                    yield res.map_err(SuperError::from);
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
    #[error(
        "Thingiverse blocked the API request,
probably due to over-use of a single HTTP header 'user-agent' value,
asking for a human-verification through HTML and JS;
You might want to set a new user_agent value in the settings,
or remove the setting all-together,
which will cause the user-agent to be automatically (re-)generated in intervals!"
    )]
    UserAgentBlocked,
    #[error("Network/Internet download failed: '{0}'")]
    DownloadError(#[from] reqwest::Error),
    #[error("Network/Internet download failed: '{0}'")]
    DownloadMiddlewareError(#[from] reqwest_middleware::Error),
    #[error("Failed to deserialize a fetched result to JSON: {0}\n\tcontent:\n{1}")]
    DeserializeAsJsonFailed(#[source] serde_json::Error, String),
    #[error(
        "Failed to deserialize a fetched JSON result to our Rust model of the expected type: {0}\n\tcontent:\n{1}"
    )]
    DeserializeFailed(#[source] serde_json::Error, String),
    #[error("Thing with ID {0} failed to fetch; API returned error: {1}")]
    HostingApiMsg(ThingId, String),
    #[error(
        "Thing with ID {0} is \"under moderation\" -> most likely flagged as copyright infringing"
    )]
    ProjectNotPublic(ThingId),
    #[error("Thing with ID {0} is proprietary/not Open Source")]
    ProjectNotOpenSource(ThingId),
    #[error("Thing with ID {0} does not exist")]
    ProjectDoesNotExist(ThingId),
}

impl From<Error> for SuperError {
    fn from(other: Error) -> Self {
        match other {
            Error::DeserializeFailed(err, content) => Self::DeserializeFailed(err, content),
            Error::DeserializeAsJsonFailed(err, content) => {
                Self::DeserializeAsJsonFailed(err, content)
            }
            Error::HostingApiMsg(thing_id, msg) => {
                Self::HostingApiMsg(format!("Thing ID {thing_id} - {msg}"))
            }
            Error::ProjectNotPublic(_) => Self::ProjectNotPublic,
            Error::ProjectNotOpenSource(thing_id) => {
                Self::ProjectNotOpenSource(Scraper::to_hosting_unit_id(thing_id))
            }
            Error::ProjectDoesNotExist(_) => Self::ProjectDoesNotExist,
            Error::IOError(err) => Self::IOError(err),
            Error::RateLimitReached => Self::RateLimitReached,
            Error::UserAgentBlocked => Self::ApiAccessBlocked(other.to_string()),
            Error::DownloadError(err) => Self::DownloadError(err),
            Error::DownloadMiddlewareError(err) => Self::DownloadMiddlewareError(err),
        }
    }
}

impl Error {
    #[must_use]
    pub const fn to_thing_state(&self) -> Option<ThingState> {
        match self {
            Self::DeserializeFailed(err, raw_api_response)
            | Self::DeserializeAsJsonFailed(err, raw_api_response) => {
                Some(ThingState::FailedToParse)
            }
            Self::HostingApiMsg(_, _) => Some(ThingState::FailedToFetch),
            Self::ProjectNotPublic(_) => Some(ThingState::Banned),
            Self::ProjectNotOpenSource(_) => Some(ThingState::Proprietary),
            Self::ProjectDoesNotExist(_) => Some(ThingState::DoesNotExist),
            Self::IOError(_)
            | Self::RateLimitReached
            | Self::UserAgentBlocked
            | Self::DownloadError(_)
            | Self::DownloadMiddlewareError(_) => None,
        }
    }

    #[must_use]
    pub const fn to_raw_api_response(&self) -> Option<&String> {
        match self {
            Self::DeserializeFailed(_, raw_api_response)
            | Self::DeserializeAsJsonFailed(_, raw_api_response) => Some(raw_api_response),
            Self::HostingApiMsg(_, _)
            | Self::ProjectNotPublic(_)
            | Self::ProjectNotOpenSource(_)
            | Self::ProjectDoesNotExist(_)
            | Self::IOError(_)
            | Self::RateLimitReached
            | Self::UserAgentBlocked
            | Self::DownloadError(_)
            | Self::DownloadMiddlewareError(_) => None,
        }
    }

    #[must_use]
    pub const fn aborts(&self) -> bool {
        self.to_thing_state().is_none()
    }
}

impl Scraper {
    #[must_use]
    pub fn to_hosting_unit_id(thing_id: ThingId) -> HostingUnitId {
        (HOSTING_PROVIDER_ID, thing_id.to_string()).into()
    }

    async fn scrape_one(
        client: Arc<ClientWithMiddleware>,
        store: &mut ThingStore,
        store_slice: &mut ThingStoreSlice,
        thing_id: ThingId,
    ) -> Result<Project, Error> {
        // We want this time to be as close as possible to the fetch,
        // but rather before then after it.
        let fetch_time = Utc::now();
        let (state, raw_api_response, thing_parsed_res) = match Self::fetch_thing(client, thing_id)
            .await?
            .into_thing_result()
        {
            Ok(state) => {
                tracing::info!("thing {thing_id} - fetched and open source");
                (ThingState::OpenSource, Some(state.1), Ok(state.0))
            }
            Err(err) => {
                let thing_state_opt = err.to_thing_state();
                if let Some(thing_state) = thing_state_opt {
                    tracing::warn!("thing {thing_id} - {thing_state} - {err}");
                    (thing_state, err.to_raw_api_response().cloned(), Err(err))
                } else {
                    tracing::error!(
                        "thing {thing_id} - fatal error; aborting thingiverse scraping: {err}"
                    );
                    return Err(err);
                }
            }
        };
        let thing_meta = ThingMeta::new(thing_id, state, fetch_time);
        store_slice.insert(thing_meta, raw_api_response).await?;
        store.set_last_scrape(fetch_time);
        let thing_parsed = thing_parsed_res?;
        let thing_file = store_slice.content_file_path(thing_id, false);
        let mut project = Project::new(Self::to_hosting_unit_id(thing_id));
        // project.crawling_meta_tree = todo!(); // TODO FIXME Set scraping meta-data!
        project.raw = Some(Chunk::from_type_and_file(
            SerializationFormat::Json,
            thing_file,
        ));
        Ok(project)
    }
}

enum ApiResponse<T: serde::de::DeserializeOwned> {
    /// Parsed as the expected type.
    Success(String, T),
    /// A JSON encoded API error response.
    Error(ThingId, TvApiError),
    /// Parsed as JSON,
    /// but could not be parsed as the specific API response type,
    /// nor as the API error type.
    Json(serde_json::Error, String),
    /// Arrived as text,
    /// but could not be parsed as valid JSON.
    Text(serde_json::Error, String),
    /// Indicates that the rate limit has been reached.
    /// No actual data was received.
    RateLimitReached,
    /// It looks like Thingiverse blocks requests
    /// depending on the HTTP header value of "user-agent
    /// when it is used too often/by multiple requesters;
    /// or so is our guess.
    UserAgentBlocked,
}

impl<T: serde::de::DeserializeOwned> ApiResponse<T> {
    fn map<NT: serde::de::DeserializeOwned>(
        self,
        success_mapper: impl FnOnce(T) -> NT,
    ) -> ApiResponse<NT> {
        match self {
            Self::Success(raw_content, success) => {
                ApiResponse::Success(raw_content, success_mapper(success))
            }
            Self::Error(thing_id, err) => ApiResponse::Error(thing_id, err),
            Self::Json(json, raw_content) => ApiResponse::Json(json, raw_content),
            Self::Text(err, raw_content) => ApiResponse::Text(err, raw_content),
            Self::RateLimitReached => ApiResponse::RateLimitReached,
            Self::UserAgentBlocked => ApiResponse::UserAgentBlocked,
        }
    }

    fn into_result(self) -> Result<(String, T), Error> {
        match self {
            Self::Success(raw_content, parsed) => Ok((raw_content, parsed)),
            Self::Error(thing_id, api_err) => Err(api_err.into_local_error(thing_id)),
            Self::Json(err, raw_content) => Err(Error::DeserializeFailed(err, raw_content)),
            Self::Text(err, raw_content) => Err(Error::DeserializeAsJsonFailed(err, raw_content)),
            Self::RateLimitReached => Err(Error::RateLimitReached),
            Self::UserAgentBlocked => Err(Error::UserAgentBlocked),
        }
    }
}

impl ApiResponse<Thing> {
    fn into_thing_result(self) -> Result<(Thing, String), Error> {
        self.into_result().and_then(|(raw_content, thing)| {
            if thing.is_open_source() {
                Ok((thing, raw_content))
            } else {
                Err(Error::ProjectNotOpenSource(thing.id))
            }
        })
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

    // #[instrument]
    fn parse_api_response<T: serde::de::DeserializeOwned>(
        thing_id: ThingId,
        content: String,
    ) -> ApiResponse<T> {
        let json_val = match serde_json::from_str::<Value>(&content) {
            Ok(json_val) => json_val,
            Err(serde_err) => {
                // if content == "You can only send 300 requests per 5 minutes. Please check https://www.thingiverse.com/developers/getting-started" {
                return if content.starts_with("You can only send ") {
                    ApiResponse::RateLimitReached
                } else if content.contains("<title>Just a moment...</title>") {
                    ApiResponse::UserAgentBlocked
                } else {
                    ApiResponse::Text(serde_err, content)
                };
            }
        };

        let err_err = match serde_json::from_value::<TvApiError>(json_val.clone()) {
            Err(err) => err,
            Ok(api_error_cont) => return ApiResponse::Error(thing_id, api_error_cont),
        };

        match serde_json::from_value::<T>(json_val) {
            Err(serde_err) => {
                tracing::warn!(
                    "Failed to parse Thingiverse API response (JSON), \
both as the expected type and as an error response:\n{serde_err}\n{err_err}"
                );
                ApiResponse::Json(serde_err, content)
            }
            Ok(parsed) => ApiResponse::Success(content, parsed),
        }
    }

    // #[instrument]
    async fn fetch_latest_thing_id(client: Arc<ClientWithMiddleware>) -> Result<ThingId, Error> {
        tracing::debug!("Fetching latest thing ID ...");

        // To try the same on the CLI:
        // curl -L \
        //     --url-query type=things \
        //     --url-query per_page=1 \
        //     --url-query sort=newest \
        //     -H "authorization: Bearer XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX" \
        //     "https://api.thingiverse.com/search/"
        let params = [("type", "things"), ("per_page", "1"), ("sort", "newest")];
        let res_raw_text =
            Self::fetch_as_text(client, "https://api.thingiverse.com/search/", &params).await?;

        Self::parse_api_response::<SearchSuccess>(ThingId::MAX, res_raw_text)
                .map(|success|
                    success.hits.first().expect(
                        "No hits returned when fetching latest thing from thingiverse.com - this should never happen"
                    ).id).into_result().map(|(raw_content, thing_id)| thing_id)
    }

    // #[instrument]
    async fn fetch_thing(
        client: Arc<ClientWithMiddleware>,
        thing_id: ThingId,
    ) -> Result<ApiResponse<Thing>, Error> {
        tracing::debug!("thing {thing_id} - fetching...");
        let params: [(&str, &str); 0] = [];
        let res_raw_text = Self::fetch_as_text(
            client,
            &format!("https://api.thingiverse.com/things/{thing_id}"),
            &params,
        )
        .await?;

        Ok(Self::parse_api_response::<Thing>(thing_id, res_raw_text))
    }
}
