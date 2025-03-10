// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures::stream::BoxStream;
use once_cell::sync::Lazy;
use reqwest::{
    header::{self, HeaderMap},
    Client,
};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use crate::{
    files_finder,
    model::{
        hosting_provider_id::HostingProviderId, hosting_type::HostingType, hosting_unit_id,
        project::Project,
    },
    settings::PartialSettings,
};

pub mod appropedia;
pub mod manifests_repo;
pub mod oshwa;
pub mod thingiverse;
pub mod thingiverse_model;
pub mod thingiverse_store;

const DEFAULT_RETRIES: u32 = 3;
const DEFAULT_TIMEOUT: u64 = 10;

pub static USER_AGENT_VALUE: Lazy<header::HeaderValue> = Lazy::new(|| {
    "okh-scraper github.com/iop-alliance/OpenKnowHow"
        .parse()
        .unwrap()
});

pub trait Config {
    fn hosting_provider(&self) -> HostingProviderId;
}

pub trait RetryConfig: Config {
    /// Number of retries for a specific fetch,
    /// e.g. a batch or single project.
    fn retries(&self) -> Option<u32>;

    /// Total timeout per request in milliseconds (ms)
    fn timeout(&self) -> Option<u64>;
}

pub trait AccessControlConfig: Config {
    fn access_token(&self) -> &str;
}

/// Serves as a common base-type for typical,
/// (de-)centralized web (HTTP) hosting platforms.
///
/// This might be git forges like GitHub,
/// Wikis like Appropedia
/// or custom hardware project hosting platforms like
/// OSHWAs or Thingiverse.
#[derive(Deserialize, Debug)]
pub struct PlatformBaseConfig {
    hosting_provider: HostingProviderId,
    // fetcher_type: String,
    /// Number of retries for a specific fetch,
    /// e.g. a batch or single project.
    retries: Option<u32>,
    /// Request timeout in milliseconds (ms)
    timeout: Option<u64>,
}

impl Config for PlatformBaseConfig {
    fn hosting_provider(&self) -> HostingProviderId {
        self.hosting_provider
    }
}

// impl DownloadCreator for PlatformBaseConfig {
//     fn create_downloader(&self) -> Arc<ClientWithMiddleware> {
//         Arc::new(
//             create_downloader(
//                 self.retries().unwrap_or(DEFAULT_RETRIES),
//                 self.timeout().unwrap_or(DEFAULT_TIMEOUT),
//             None))
//     }
// }

impl RetryConfig for PlatformBaseConfig {
    fn retries(&self) -> Option<u32> {
        self.retries
    }

    fn timeout(&self) -> Option<u64> {
        self.timeout
    }
}

/// same like [`PlatformBaseConfig`] but with access control.
#[derive(Deserialize, Debug)]
pub struct ACPlatformBaseConfig {
    hosting_provider: HostingProviderId,
    // fetcher_type: String,
    /// Number of retries for a specific fetch,
    /// e.g. a batch or single project.
    retries: Option<u32>,
    /// Request timeout in milliseconds (ms)
    timeout: Option<u64>,
    /// Batch size when fetching multiple items (e.g. projects)
    batch_size: Option<usize>,
    access_token: String,
}

impl Config for ACPlatformBaseConfig {
    fn hosting_provider(&self) -> HostingProviderId {
        self.hosting_provider
    }
}

// impl DownloadCreator for ACPlatformBaseConfig {
//     fn create_downloader(&self) -> Arc<ClientWithMiddleware> {
//         let authorization = Some(format!("Bearer {}", self.access_token()));
//         Arc::new(
//             create_downloader(
//                 self.retries().unwrap_or(DEFAULT_RETRIES),
//                 self.timeout().unwrap_or(DEFAULT_TIMEOUT),
//             Some(create_headers(authorization))))
//     }
// }

impl RetryConfig for ACPlatformBaseConfig {
    fn retries(&self) -> Option<u32> {
        self.retries
    }

    fn timeout(&self) -> Option<u64> {
        self.timeout
    }
}

impl AccessControlConfig for ACPlatformBaseConfig {
    fn access_token(&self) -> &str {
        &self.access_token
    }
}

/// Thrown when creating a new [`Scraper`] failed.
#[derive(Error, Debug)]
pub enum CreationError {
    #[error("Unknown fetcher type: '{0}'")]
    UnknownFetcherType(String),
    #[error("Invalid config for fetcher type '{0}': {1}")]
    InvalidConfig(String, Value),
}

/// Thrown when a [`Scraper`] failed to scrape in general,
/// or while trying to scrape a single or a batch of projects.
#[derive(Error, Debug)]
pub enum Error {
    // #[error("Unknown fetcher type: '{0}'")]
    // UnknownFetcherType(String),
    // #[error("Invalid config for fetcher type '{0}': {1}")]
    // InvalidConfig(String, Value),
    #[error("Failed to clone a git repo (synchronously): '{0}'")]
    FailedGitClone(#[from] git2::Error),
    #[error("Some I/O problem: '{0}'")]
    IOError(#[from] std::io::Error), // TODO Too low level to be here, and no circumstances info
    #[error("Failed to fetch a git repo (asynchronously): '{0}'")]
    FailedGitFetch(#[from] asyncgit::Error),
    #[error("Error while searching files in a local directory: '{0}'")]
    FindError(#[from] files_finder::FindError),
    #[error("Network/Internet download failed: '{0}'")]
    DownloadError(#[from] reqwest::Error),
    #[error("Network/Internet download failed: '{0}'")]
    DownloadMiddlewareError(#[from] reqwest_middleware::Error),
    #[error("{0} reached (and very likely surpassed) a total number of projects that is higher than the max fetch-limit set in its API ({1}); please inform the {0} admins!")]
    FetchLimitReached(HostingProviderId, usize),
    #[error("Failed to deserialize a fetched (supposed) JSON result to our Rust model of it: {0}")]
    DeserializeFailed(#[from] serde_json::Error),
    #[error("OSHWA API returned error content: {0}")]
    OshwaApiError(#[from] oshwa::ApiError), // TODO Really, we should not have such scraper-specific errors here
    #[error("Thingiverse search API returned error: {0}")]
    ThingiverseSearchError(#[from] thingiverse_model::SearchError), // TODO Really, we should not have such scraper-specific errors here
    #[error("Failed to parse a hosting URL to a hosting-unit-id: {0}")]
    HostingUnitIdParseError(#[from] hosting_unit_id::ParseError),
    #[error("Failed pull git repo (asynchronously): {0}")]
    GitAsyncPullError(#[from] crossbeam_channel::RecvError),
}

/// Contains descriptive data about the type of a scraper.
pub struct TypeInfo {
    /// Machine-readable name/id of this type of scraper.
    /// It should be in "kebab-case".
    name: &'static str,

    /// Human-readable description of this type of scraper.
    description: &'static str,

    hosting_type: HostingType,
}

/// Creates instances of scrapers of a specific type.
pub trait Factory {
    /// Info about the type of fetchers produced by this factory.
    fn info(&self) -> &'static TypeInfo;

    /// Creates a new instance of this type of fetcher,
    /// following the supplied configuration.
    fn create(
        &self,
        config_all: Arc<PartialSettings>,
        config_fetcher: Value,
    ) -> Result<Box<dyn Scraper>, CreationError>;
}

/// A scraper of a specific type,
/// usually tailored to scrape projects from a specific hosting technology.
#[async_trait(?Send)]
pub trait Scraper {
    /// Info about this type of scraper.
    fn info(&self) -> &'static TypeInfo;

    async fn fetch_all(&self) -> BoxStream<'static, Result<Project, Error>>;
}

/// Creates a default set of headers for downloads.
/// @param authorization This is not the bare access token,
///   but already has to contain the "Bearer " prefix
fn create_headers(authorization: Option<String>) -> header::HeaderMap {
    let mut headers = header::HeaderMap::new();
    headers.insert(header::USER_AGENT, USER_AGENT_VALUE.clone());
    if let Some(access_token_val) = authorization {
        // Consider marking security-sensitive headers with `set_sensitive`.
        let mut auth_value = header::HeaderValue::from_str(&access_token_val)
            .expect("Invalid HTTP Authorization/access-token value");
        auth_value.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth_value);
    }
    headers
}

/// Creates a new [`reqwest::Client`] with the supplied retry and timeout settings.
/// @param retries Number of retries for a single fetch
/// @param timeout Total timeout per request in milliseconds (ms)
fn create_downloader(
    retries: u32,
    timeout: u64,
    headers: Option<header::HeaderMap>,
) -> ClientWithMiddleware {
    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(retries);
    let mut client_builder = Client::builder().timeout(Duration::from_millis(timeout));
    if let Some(headers_val) = headers {
        client_builder = client_builder.default_headers(headers_val);
    }
    ClientBuilder::new(client_builder.build().unwrap())
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build()
}

pub fn create_downloader_retry(config: &impl RetryConfig) -> Arc<ClientWithMiddleware> {
    Arc::new(create_downloader(
        config.retries().unwrap_or(DEFAULT_RETRIES),
        config.timeout().unwrap_or(DEFAULT_TIMEOUT),
        None,
    ))
}

pub fn create_downloader_ac(config: &impl AccessControlConfig) -> Arc<ClientWithMiddleware> {
    let authorization = Some(format!("Bearer {}", config.access_token()));
    Arc::new(create_downloader(
        DEFAULT_RETRIES,
        DEFAULT_TIMEOUT,
        Some(create_headers(authorization)),
    ))
}

pub fn create_downloader_retry_ac<T: RetryConfig + AccessControlConfig>(
    config: &T,
) -> Arc<ClientWithMiddleware> {
    let authorization = Some(format!("Bearer {}", config.access_token()));
    Arc::new(create_downloader(
        config.retries().unwrap_or(DEFAULT_RETRIES),
        config.timeout().unwrap_or(DEFAULT_TIMEOUT),
        Some(create_headers(authorization)),
    ))
}

// pub fn assemble_fetcher_factories() -> HashMap<String, impl FetcherFactory> {
//     let fetchers = vec![oshwa::FetcherFactory, appropedia::FetcherFactory];
//     fetchers.into_iter().map(|f| (f.name().to_string(), f)).collect()
// }

#[must_use]
pub fn assemble_factories() -> HashMap<String, Box<dyn Factory>> {
    let fetchers: Vec<Box<dyn Factory>> = vec![
        Box::new(oshwa::ScraperFactory),
        Box::new(appropedia::ScraperFactory),
        Box::new(manifests_repo::ScraperFactory),
        Box::new(thingiverse::ScraperFactory),
    ];
    fetchers
        .into_iter()
        .map(|f| (f.info().name.to_string(), f))
        .collect()
}

// macro_rules! yield_err {
//     ($res:expr) => {
//         match $res {
//             Err(err) => yield Err(err.into()),
//             Ok(value) => {},
//         }
//     };
// }

// pub(crate) use yield_err;

macro_rules! ok_or_return_err_stream {
    ($res:expr) => {
        match $res {
            Err(err) => {
                return stream! {
                    yield Err(err.into())
                }
                .boxed()
            }
            Ok(value) => value,
        }
    };
}

pub(crate) use ok_or_return_err_stream;
