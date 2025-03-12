// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::create_downloader_retry_ac;
use super::thingiverse_model::{SearchError, SearchSuccess};
use super::{
    thingiverse_store::ThingId, ACPlatformBaseConfig, CreationError, Error,
    Factory as IScraperFactory, Scraper as IScraper, TypeInfo,
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
        thingiverse_model::Thing,
        thingiverse_store::{ThingMeta, ThingState, ThingStore},
    },
    settings::PartialSettings,
    structured_content::{Chunk, RawContent, SerializationFormat},
    tools::{SpdxLicenseExpression, LICENSE_UNKNOWN, USER_AGENT_VALUE},
};
use async_std::{fs::File, io};
use async_stream::stream;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fs4::async_std::AsyncFileExt;
use futures::{stream::BoxStream, stream::StreamExt};
use governor::{Quota, RateLimiter};
use reqwest::header::{HeaderMap, AUTHORIZATION, USER_AGENT};
use reqwest_middleware::ClientWithMiddleware;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;
use std::{borrow::Cow, fmt::Display, rc::Rc, sync::Arc};
use tokio::time::Duration;
use tracing::instrument;

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
    // fetcher_type: String,
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
        config_fetcher: Value,
    ) -> Result<Box<dyn IScraper>, CreationError> {
        let config: Config = serde_json::from_value(config_fetcher).unwrap();
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

    async fn fetch_all(&self) -> BoxStream<'static, Result<Project, Error>> {
        let client = Arc::<_>::clone(&self.downloader);

        let latest_thing_id = match Self::fetch_latest_thing_id(client.clone()).await {
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

        tracing::info!("Preparing disc store ...");

        let store_res = ThingStore::new(
            async_std::path::PathBuf::from(self.config.things_store_root.clone()),
            thing_id_range_min,
            thing_id_range_max,
        )
        .await;

        // let mut store = match store_res {
        //     Err(err) => return stream! {
        //         yield Err(err.into())
        //     }
        //     .boxed(),
        //     Ok(store) => {
        //         store
        //     }
        // };
        let mut store = ok_or_return_err_stream!(store_res);

        tracing::info!("Fetching {} - total ...", self.info().name);
        stream! {
            loop {
                let mut store_slice = store.get_next_slice().await?;
                let mut store_slice = store_slice.write().await;
                let previously_os_things = store_slice.cloned_os(); // TODO Once we have an initial scrape, we should implement scraping these again, after this loop that scraped the yet untried
                while let Some(thing_meta) = store_slice.next(ThingState::Untried) {
                    let thing_id = thing_meta.get_id();
                    // We want this time to be as close as possible to the fetch,
                    // but rather before then after it.
                    let fetch_time = Utc::now();
                    let (state, raw_api_response) = match Self::fetch_thing(client.clone(), thing_id).await {
                    // let (state, raw_api_response) = match Ok::<(String, &str), Error>(("".to_string(), "")) {
                        Err(err) => {
                            if let Error::ProjectDoesNotExist | Error::ProjectDoesNotExistId(_) = err {
                                tracing::info!("Thing {thing_id} does not exist");
                                (ThingState::DoesNotExist, None)
                            } else if let Error::ProjectNotPublic = err {
                                tracing::info!("Thing {thing_id} is \"under moderation\" -> most likely flagged as copyright infringing");
                                (ThingState::Banned, None)
                            } else {
                                tracing::error!("Failed to fetch thing-ID {thing_id}: {err}");
                                (ThingState::FailedToFetch, None)
                            }
                        },
                        Ok((raw_api_response, parsed_as_thing)) => {
                            let is_os = parsed_as_thing.is_open_source();
                            if is_os {
                                (ThingState::OpenSource, Some(raw_api_response))
                            } else {
                                (ThingState::Proprietary, None)
                            }
                        }
                    };
                    let thing_meta = ThingMeta::new(thing_id, state, fetch_time);
                    if let Err(err) = store_slice.insert(thing_meta, raw_api_response.clone()).await{
                        yield Err(err.into());
                    }
                    store.set_last_scrape(fetch_time);
                    if let Some(raw_api_response_val) = raw_api_response {
                        let mut project = Project::new(HostingUnitId::WebById(HostingUnitIdWebById::from_parts(
                            HOSTING_PROVIDER_ID,
                            thing_id.to_string())
                        ));
                        // project.crawling_meta_tree = todo!(); // TODO FIXME Set meta-data!
                        project.raw = Some(Chunk::from_content(SerializationFormat::Json, RawContent::String(raw_api_response_val)));
                        yield Ok(project);
                    }
                }
            }
        }.boxed()
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
        Ok(client
            // .lock()
            // .unwrap()
            .get(url)
            .query(&params)
            .send()
            .await?
            .text()
            .await?)
    }

    // #[instrument]
    fn parse_api_response<T: serde::de::DeserializeOwned>(content: &str) -> Result<T, Error> {
        serde_json::from_str::<T>(content).map_err(|err| {
            // tracing::debug!("Trying to parse API response as Error ...");
            let res_api_error_cont = serde_json::from_str::<SearchError>(content);
            match res_api_error_cont {
                Err(err_err) => {
                    tracing::warn!(
                        "Failed to parse Thingiverse API response (assumed JSON), \
both as the expected type and as an error response:\n{err}\n{err_err}"
                    );
                    tracing::warn!("... raw, (assumed JSON) value:\n{content}");
                    err.into()
                    // err_err.into()
                }
                Ok(api_error_cont) => api_error_cont.into(),
            }
        })
    }

    // #[instrument]
    async fn fetch_latest_thing_id(client: Arc<ClientWithMiddleware>) -> Result<ThingId, Error> {
        tracing::info!("Fetching latest thing ID ...");

        let params = [("type", "things"), ("per_page", "1"), ("sort", "newest")];
        let res_raw_text =
            Self::fetch_as_text(client, "https://api.thingiverse.com/search/", &params).await?;
        // let res_raw_text = client
        //     // .lock()
        //     // .unwrap()
        //     .get("https://api.thingiverse.com/search/")
        //     .query(&params)
        //     .send()
        //     .await?
        //     .text()
        //     .await?;

        Self::parse_api_response::<SearchSuccess>(&res_raw_text).map(|success| success.hits.first().expect("No hits returned when fetching latest thing from thingiverse.com - this should never happen").id)
    }

    #[instrument]
    // async fn fetch_thing(&self, thing_id: ThingId) -> Result<(String, Thing), Error> {
    async fn fetch_thing(
        client: Arc<ClientWithMiddleware>,
        thing_id: ThingId,
    ) -> Result<(String, Thing), Error> {
        tracing::info!("Fetching thing {thing_id} ...");
        // let client = Arc::<_>::clone(&self.downloader);
        // let res_raw_text = client
        //     // .lock()
        //     // .unwrap()
        //     .get(&format!("https://api.thingiverse.com/things/{thing_id}"))
        //     .send()
        //     .await?
        //     .text()
        //     .await?;
        let params: [(&str, &str); 0] = [];
        let res_raw_text = Self::fetch_as_text(
            client,
            &format!("https://api.thingiverse.com/things/{thing_id}"),
            &params,
        )
        .await?;

        Self::parse_api_response::<Thing>(&res_raw_text).map(|thing| (res_raw_text, thing))
    }
}
