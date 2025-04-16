// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{create_downloader_retry_ac, AccessControlConfig, RetryConfig};
use super::{
    Config as IConfig, CreationError, Error, Factory as IScraperFactory, Scraper as IScraper,
    TypeInfo,
};
use crate::{
    model::{
        hosting_provider_id::HostingProviderId, hosting_type::HostingType,
        hosting_unit_id::HostingUnitId, project::Project,
    },
    settings::PartialSettings,
};
use async_stream::stream;
use async_trait::async_trait;
use futures::{stream::BoxStream, stream::StreamExt};
use governor::{Quota, RateLimiter};
use model::{ErrorCont, Projects};
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;
use tokio::time::Duration;
use tracing::instrument;

mod model;

pub const HOSTING_PROVIDER_ID: HostingProviderId = HostingProviderId::OshwaOrg;

pub static SCRAPER_TYPE: LazyLock<TypeInfo> = LazyLock::new(|| TypeInfo {
    name: "oshwa",
    description: "Fetches projects from Open Source HardWare Association (OSHWA),
(<https://certification.oshwa.org/>), via their API.",
    hosting_type: HostingType::Oshwa,
});

/// This has to be static,
/// because if we created multiple instances of [`Scraper`],
/// we would send too many requests from the same network address.
pub static RATE_LIMITER: LazyLock<Arc<super::RL>> = LazyLock::new(|| {
    Arc::new(RateLimiter::direct(
        Quota::with_period(Duration::from_secs(5)).unwrap(),
    ))
});

const DEFAULT_BATCH_SIZE: usize = 100;

/// Like [`super::ACPlatformBaseConfig`].
#[derive(Deserialize, Debug)]
pub struct Config {
    /// Number of retries for a specific fetch,
    /// e.g. a batch or single project.
    retries: Option<u32>,
    /// Request timeout in milliseconds (ms)
    timeout: Option<u64>,
    /// Batch size when fetching multiple items (e.g. projects)
    batch_size: Option<usize>,
    access_token: String,
}

impl IConfig for Config {
    fn hosting_provider(&self) -> HostingProviderId {
        HOSTING_PROVIDER_ID
    }
}

impl RetryConfig for Config {
    fn retries(&self) -> Option<u32> {
        self.retries
    }

    fn timeout(&self) -> Option<u64> {
        self.timeout
    }
}

impl AccessControlConfig for Config {
    fn access_token(&self) -> &str {
        &self.access_token
    }
}

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
        let config: Config = serde_json::from_value(config_scraper).unwrap();
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
    downloader: Arc<ClientWithMiddleware>, // TODO Check Is the Arc really still required, now that each scraper has its own downloader (before there was one shared by the whole application) -> if changed, change in all other scrapers too!
}

#[async_trait(?Send)]
impl IScraper for Scraper {
    fn info(&self) -> &'static TypeInfo {
        &SCRAPER_TYPE
    }

    async fn scrape(&self) -> BoxStream<'static, Result<Project, Error>> {
        let access_token = self.config.access_token.clone();

        tracing::info!("Fetching {} - total ...", self.info().name);
        let res_projects =
            Self::fetch_projects_batch(Arc::<_>::clone(&self.downloader), &access_token, 1, 0)
                .await;
        match res_projects {
            Err(err) => stream! {
                yield Err(err)
            }
            .boxed(),
            Ok(projects_probe) => {
                let total = projects_probe.total;
                let batch_size = self.config.batch_size.unwrap_or(DEFAULT_BATCH_SIZE);
                let batches = total / batch_size;
                let self_name = self.info().name;
                let client = Arc::<_>::clone(&self.downloader);

                stream! {
                    for batch_index in 0..batches {
                        let rate_limiter = Arc::<_>::clone(&RATE_LIMITER);
                        tracing::info!(
                            "Fetching {self_name} - batch {batch_index}/{batches} ..."
                        );
                        rate_limiter.until_ready().await;
                        let offset = batch_index * batch_size;
                        let batch_res = Self::fetch_projects_batch(Arc::<_>::clone(&client), &access_token, batch_size, offset).await;
                        match batch_res {
                            Err(err) => yield Err(err),
                            Ok(projects) => {
                                for raw_proj in projects.items {
                                    yield Ok(Project::new(HostingUnitId::from((HOSTING_PROVIDER_ID, raw_proj.oshwa_uid))));
                                }
                            },
                        }
                    }
                }.boxed()
            }
        }
    }
}

impl Scraper {
    #[instrument]
    async fn fetch_projects_batch(
        client: Arc<ClientWithMiddleware>,
        access_token: &str,
        batch_size: usize,
        offset: usize,
    ) -> Result<Projects, Error> {
        let params = [("limit", batch_size), ("offset", offset)];
        let raw_text = client
            .get("https://certificationapi.oshwa.org/api/projects")
            .query(&params)
            .send()
            .await?
            .text()
            .await?;

        let res_raw_json = serde_json::from_str::<Value>(&raw_text);
        let json_val = match res_raw_json {
            Ok(json_val) => json_val,
            Err(serde_err) => {
                tracing::warn!("Failed to parse OSHWA API response as JSON:\n{serde_err}");
                return Err(Error::DeserializeAsJson(serde_err, raw_text));
            }
        };

        let err_err = match serde_json::from_value::<ErrorCont>(json_val.clone()) {
            Err(err) => err,
            Ok(api_error_cont) => {
                return Err(Error::HostingApiMsg(api_error_cont.error.to_string()))
            }
        };

        match serde_json::from_value::<Projects>(json_val) {
            Err(serde_err) => {
                tracing::warn!(
                    "Failed to parse OSHWA API response (JSON), \
both as the expected type and as an error response:\n{serde_err}\n{err_err}"
                );
                Err(Error::Deserialize(serde_err, raw_text))
            }
            Ok(parsed) => Ok(parsed),
        }
    }
}
