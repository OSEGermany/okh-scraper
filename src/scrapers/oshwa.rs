// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{create_downloader_retry_ac, RetryConfig};
use super::{
    ACPlatformBaseConfig, CreationError, Error, Factory as IScraperFactory, Scraper as IScraper,
    TypeInfo,
};
use crate::{
    model::{
        hosting_provider_id::HostingProviderId, hosting_type::HostingType,
        hosting_unit_id::HostingUnitId, project::Project,
    },
    settings::PartialSettings,
    tools::{SpdxLicenseExpression, LICENSE_UNKNOWN, USER_AGENT_VALUE},
};
use async_stream::stream;
use async_trait::async_trait;
use futures::{stream::BoxStream, stream::StreamExt};
use governor::{Quota, RateLimiter};
use once_cell::sync::Lazy;
use reqwest::header::{HeaderMap, AUTHORIZATION, USER_AGENT};
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use serde_json::Value;
use std::{borrow::Cow, fmt::Display, rc::Rc, sync::Arc};
use tokio::time::Duration;
use tracing::instrument;

pub const HOSTING_PROVIDER_ID: HostingProviderId = HostingProviderId::OshwaOrg;

pub static SCRAPER_TYPE: Lazy<TypeInfo> = Lazy::new(|| TypeInfo {
    name: "oshwa",
    description: "Fetches projects from Open Source HardWare Association (OSHWA),
(<https://certification.oshwa.org/>), via their API.",
    hosting_type: HostingType::Oshwa,
});

/// This has to be static,
/// because if we created multiple instances of [`Scraper`],
/// we would send too many requests from the same network address.
pub static RATE_LIMITER: Lazy<
    Arc<
        RateLimiter<
            governor::state::NotKeyed,
            governor::state::InMemoryState,
            governor::clock::QuantaClock,
            governor::middleware::NoOpMiddleware<governor::clock::QuantaInstant>,
        >,
    >,
> = Lazy::new(|| {
    Arc::new(RateLimiter::direct(
        Quota::with_period(Duration::from_secs(5)).unwrap(),
    ))
});

const DEFAULT_BATCH_SIZE: usize = 100;

type Config = ACPlatformBaseConfig;

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
    downloader: Arc<ClientWithMiddleware>, // TODO Check Is the Arc really still required, now that each scraper has its own downloader (before there was one shared by the whole application) -> if changed, change in all other scrapers too!
}

// impl Fetcher {
//     // #[instrument]
//     async fn fetch_all_XXXXXXXXXX(&self) -> Box<dyn Stream<Item = Result<Project, Error>>> {
//         let rate_limiter = Arc::new(RateLimiter::direct(
//             Quota::with_period(Duration::from_secs(5)).unwrap(),
//         ));
//         let client: Arc<Mutex<reqwest::Client>> = Arc::new(Mutex::new(reqwest::Client::new()));
//         let access_token = self.config.access_token.clone();

//         tracing::info!("Fetching {} - total ...", self.info().name);
//         let res_projects = Self::fetch_projects_batch(client.clone(), &access_token, 1, 0).await;
//         // Box::new(stream! {
//         match res_projects {
//             Err(err) => Box::new(stream! {
//                 yield Err(err)
//             }),
//             Ok(projects_probe) => {
//                 let total = projects_probe.total;
//                 let batch_size = self.config.batch_size.unwrap_or(DEFAULT_BATCH_SIZE);
//                 // let mut fetches = Vec::new();
//                 let batches = total / batch_size;
//                 let self_name = self.info().name;
//                 Box::new(
//                     stream! {
//                         for batch_index in 0..batches {
//                             let rate_limiter = rate_limiter.clone();
//                             let client = client.clone();
//                             // let client = reqwest::Client::new();
//                             // fetches.push(
//                             //     async move {
//                                 tracing::info!(
//                                     "Fetching {self_name} - batch {batch_index}/{batches} ..."
//                                 );
//                                 rate_limiter.until_ready().await;
//                                 // let resp = reqwest::get(&url).await?;
//                                 // tracing::debug!("Status for {}: {}", url, resp.status());
//                                 // Result::<_, reqwest::Error>::Ok(())
//                                 let offset = batch_index * batch_size;
//                                 let batch_res = Self::fetch_projects_batch(client, &access_token, batch_size, offset).await;
//                                 match batch_res {
//                                     Err(err) => yield Err(err.into()),
//                                     Ok(projects) => {
//                                         for raw_proj in projects.items {
//                                             yield Ok(Project::new(HostingUnitId::from((HOSTING_PROVIDER_ID, raw_proj.oshwa_uid))));
//                                         }
//                                     },
//                                 }
//                         }
//                     }
//                 )
//             }
//         }
//     }
// }

#[async_trait(?Send)]
impl IScraper for Scraper {
    fn info(&self) -> &'static TypeInfo {
        &SCRAPER_TYPE
    }

    async fn fetch_all(&self) -> BoxStream<'static, Result<Project, Error>> {
        // let client: Arc<Mutex<reqwest::Client>> = Arc::new(Mutex::new(reqwest::Client::new()));
        // let client = reqwest::Client::new();
        let access_token = self.config.access_token.clone();

        tracing::info!("Fetching {} - total ...", self.info().name);
        // let res_projects = Self::fetch_projects_batch(client.clone(), &access_token, 1, 0).await;
        let res_projects =
            Self::fetch_projects_batch(Arc::<_>::clone(&self.downloader), &access_token, 1, 0)
                .await;
        // Box::new(stream! {
        match res_projects {
            Err(err) => stream! {
                yield Err(err)
            }
            .boxed(),
            Ok(projects_probe) => {
                let total = projects_probe.total;
                let batch_size = self.config.batch_size.unwrap_or(DEFAULT_BATCH_SIZE);
                // let mut fetches = Vec::new();
                let batches = total / batch_size;
                let self_name = self.info().name;
                let client = Arc::<_>::clone(&self.downloader);
                // let client = self.config_all.downloader;

                stream! {
                        for batch_index in 0..batches {
                            let rate_limiter = Arc::<_>::clone(&RATE_LIMITER);
                            // let client = reqwest::Client::new();
                            // fetches.push(
                            //     async move {
                                tracing::info!(
                                    "Fetching {self_name} - batch {batch_index}/{batches} ..."
                                );
                                rate_limiter.until_ready().await;
                                // let resp = reqwest::get(&url).await?;
                                // tracing::debug!("Status for {}: {}", url, resp.status());
                                // Result::<_, reqwest::Error>::Ok(())
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
                        //     }
                        // );
                        }
                    }.boxed()

                // join_all(fetches).await
                // let pages: Result<Vec<Vec<String>>, Error> = join_all(fetches).await.into_iter().collect();

                // let pages: Vec<Result<Vec<String>, Error>> = join_all(fetches).await;
                // let pages_collected: (Vec<String>, Vec<Error>) =
                //     pages
                //         .into_iter()
                //         .fold((Vec::new(), Vec::new()), |acc, res| {
                //             match res {
                //                 Ok(projects) => acc.0.append(&mut projects),
                //                 Err(err) => acc.1.push(err),
                //             }
                //             acc
                //         });
            }
        }
        // let projects: Result<Vec<String>, Error> = pages.map(|bla| bla.into_iter().flatten().collect());

        // fetch_batch(50, 0).await
        // Ok(pages_collected)
    }
}

#[derive(Deserialize, Debug)]
struct OshwaProject {
    #[serde(rename = "oshwaUid")]
    oshwa_uid: String,
}

#[derive(Deserialize, Debug)]
struct Projects {
    items: Vec<OshwaProject>,
    limit: usize,
    total: usize,
}

#[derive(Deserialize, Debug)]
struct ErrorDetail {
    msg: String,
    param: String,
    location: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    status_code: usize,
    error_code: String,
    message: String,
    details: Vec<ErrorDetail>,
}

impl Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:#?}")
    }
}

impl std::error::Error for ApiError {}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ErrorCont {
    error: ApiError,
}

#[derive(Deserialize, Debug)]
enum ResponsiblePartyType {
    Company,
    Organization,
    Individual,
}

#[derive(Deserialize, Debug, Clone, Copy)]
enum Category {
    Agriculture,
    Arts,
    Education,
    Electronics,
    Environmental,
    Iot,
    Manufacturing,
    Other,
    Science,
    Tool,
    Wearables,

    #[serde(rename = "3D Printing")]
    _3DPrinting,
    Enclosure,
    #[serde(rename = "Home Connection")]
    HomeConnection,
    Robotics,
    Sound,
    Space,
}

impl Category {
    pub const fn to_cpc(self) -> Option<&'static str> {
        match self {
            Self::_3DPrinting => Some("B33Y"),
            Self::Enclosure => Some("F16M"),
            Self::HomeConnection => Some("H04W"),
            Self::Robotics => Some("B25J9/00"),
            Self::Sound => Some("H04R"),
            Self::Space => Some("B64G"),

            Self::Agriculture
            | Self::Arts
            | Self::Education
            | Self::Electronics
            | Self::Environmental
            | Self::Iot
            | Self::Manufacturing
            | Self::Other
            | Self::Science
            | Self::Tool
            | Self::Wearables => None,
        }
    }
}

/// License IDs as found on OSHWA.
///
/// NOTE: It is important to keep the serde names
/// consistent with what OSHWA uses,
/// _not_ with SPDX!
#[derive(Deserialize, Clone, Copy, Debug)]
enum OshwaLicense {
    #[serde(rename = "None")]
    None,
    #[serde(rename = "Other")]
    Other,
    #[serde(rename = "BSD-2-Clause")]
    Bsd2Clause,
    #[serde(rename = "CC0-1.0", alias = "CC 0")]
    Cc0_1_0,
    #[serde(rename = "CC-BY-4.0", alias = "CC BY")]
    CcBy4_0,
    #[serde(rename = "CC-BY-SA-4.0", alias = "CC BY-SA")]
    CcBySa4_0,
    #[serde(rename = "CERN OHL", alias = "CERN")]
    CernOhl,
    #[serde(rename = "GPL")]
    Gpl,
    #[serde(rename = "GPL-3.0")]
    Gpl3_0,
    Solderpad,
    #[serde(rename = "TAPR", alias = "OHL")]
    TaprOhl,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ApiProject {
    responsible_party_type: ResponsiblePartyType,
    responsible_party: String,
    public_contact: String,
    project_name: String,
    project_version: String,
    project_description: String,
    #[serde(rename = "oshwaUid")]
    oshwa_uid: String,
    hardware_license: Option<OshwaLicense>,
    documentation_license: Option<OshwaLicense>,
    software_license: Option<OshwaLicense>,
    primary_type: Category,
    additional_type: Category,
    country: String,
    certification_date: String, // TODO parse as "%Y-%m-%dT%H:%M%z"
}

impl ApiProject {
    pub fn license(&self) -> Option<SpdxLicenseExpression> {
        let mut license = self.hardware_license;

        if license.is_none() {
            return Some(LICENSE_UNKNOWN);
        }

        if matches!(license, Some(OshwaLicense::Other)) {
            license = self.documentation_license;
        }

        if license.is_none()
            || matches!(license, Some(OshwaLicense::Other))
            || matches!(license, Some(OshwaLicense::None))
        {
            return Some(LICENSE_UNKNOWN);
        }

        license.and_then(OshwaLicense::to_spdx_expr).map(Cow::from)
    }
}

impl OshwaLicense {
    pub const fn to_spdx_expr(self) -> Option<&'static str> {
        Some(match self {
            Self::Bsd2Clause => "BSD-2-Clause",
            // Self::CC0 => "CC0-1.0",
            Self::Cc0_1_0 => "CC0-1.0",
            // Self::CC_BY => "CC-BY-4.0",
            // Self::CC_BY_SA => "CC-BY-SA-4.0",
            Self::CcBy4_0 => "CC-BY-4.0",
            Self::CcBySa4_0 => "CC-BY-SA-4.0",
            // Self::CERN => "CERN-OHL-1.2",
            Self::CernOhl => "CERN-OHL-1.2",
            Self::Gpl => "GPL-3.0-or-later",
            Self::Gpl3_0 => "GPL-3.0-only",
            Self::Solderpad => "Apache-2.0 WITH SHL-2.1",
            Self::TaprOhl => "TAPR-OHL-1.0",
            Self::None | Self::Other => return None,
        })
    }
}

// impl Projects {
//     pub fn check_limit(&self) -> BoxResult<()> {
//         if self.query.category_members.len() == self.limits.category_members {
//             return Err(format!("Appropedia reached (and very likely surpassed) a total number of projects that is higher than the max fetch limit set in its API ({}); please inform the appropedia.org admins!", self.limits.category_members).into());
//         }
//         Ok(())
//     }
// }

impl From<Projects> for Vec<String> {
    fn from(value: Projects) -> Self {
        value.items.iter().map(|p| p.oshwa_uid.clone()).collect()
    }
}

impl Scraper {
    #[instrument]
    async fn fetch_projects_batch(
        // client: Arc<Mutex<reqwest::Client>>,
        // client: Arc<Box<reqwest::Client>>,
        client: Arc<ClientWithMiddleware>,
        // client: &Box<reqwest::Client>,
        // client: Arc<reqwest::Client>,
        // client: &reqwest::Client,
        access_token: &str,
        batch_size: usize,
        offset: usize,
    ) -> Result<Projects, Error> {
        let params = [("limit", batch_size), ("offset", offset)];
        let mut headers = HeaderMap::new();
        // headers.insert(ACCEPT, "application/json".parse().unwrap());
        headers.insert(USER_AGENT, USER_AGENT_VALUE.clone());
        headers.insert(
            AUTHORIZATION,
            format!("Bearer {access_token}").parse().unwrap(),
        );
        let res_projects_raw_json = client
            // .lock()
            // .unwrap()
            .get("https://certificationapi.oshwa.org/api/projects")
            .headers(headers)
            .query(&params)
            .send()
            .await?
            .json::<Value>()
            .await?;

        serde_json::from_value::<Projects>(res_projects_raw_json.clone()).map_err(|_err| {
            tracing::debug!(
                "Trying to parse OSHWA (supposed) error JSON return:\n{res_projects_raw_json}"
            );
            let res_api_error_cont = serde_json::from_value::<ErrorCont>(res_projects_raw_json);
            match res_api_error_cont {
                Err(deserialize_err) => deserialize_err.into(),
                Ok(api_error_cont) => api_error_cont.error.into(),
            }
        })
    }

    // async fn fetch_batch(
    //     client: Arc<Mutex<reqwest::Client>>,
    //     access_token: &str,
    //     batch_size: usize,
    //     offset: usize,
    // ) -> Result<Vec<String>, Error> {
    //     let res_projects = Self::fetch_projects_batch(client, access_token, batch_size, offset).await?;
    //     // res_projects.check_limit()?;
    //     let project_titles: Vec<String> = res_projects.into();
    //     tracing::debug!("{:#?}", project_titles);
    //     Ok(project_titles)
    // }
}

// struct Page {
//     total: usize,
//     batch: usize,
//     offset: usize,
// }

// struct PagesIterator<T> {
//     total: usize,
//     batch: usize,
//     offset: usize,
// }

// impl<T> Iterator for PagesIterator<T> {
//     type Item = T;

//     fn next(&mut self) -> Option<T> {
//         let result = match self.index {
//             0 => self.pixel.r,
//             1 => self.pixel.g,
//             2 => self.pixel.b,
//             _ => return None,
//         };
//         self.index += 1;
//         Some(|url| {
//             let rate_limiter = rate_limiter.clone();
//             async move {
//                 rate_limiter.until_ready().await;
//                 let resp = reqwest::get(&url).await?;
//                 tracing::debug!("Status for {}: {}", url, resp.status());
//                 Result::<_, reqwest::Error>::Ok(())
//             }
//         })
//     }
// }

// def fetch_all(self, start_over=True) -> Generator[FetchResult]:
// last_offset = 0
// num_fetched = 0
// batch_size = self.BATCH_SIZE
// if start_over:
//     self._state_repository.delete(__hosting_id__)
// else:
//     state = self._state_repository.load(__hosting_id__)
//     if state:
//         last_offset = state.get("last_offset", 0)
//         num_fetched = state.get("num_fetched", 0)

// while True:
//     log.debug("fetching projects %d to %d", num_fetched, num_fetched + batch_size)

//     self._rate_limit.apply()
//     response = self._session.get(
//         url="https://certificationapi.oshwa.org/api/projects",
//         params={
//             "limit": batch_size,
//             "offset": last_offset
//         },
//     )
//     self._rate_limit.update()
//     if response.status_code > 205:
//         raise FetcherError(f"failed to fetch projects from {__hosting_id__}: {response.text}")

//     data = response.json()
//     last_visited = datetime.now(timezone.utc)
//     for raw_project in data["items"]:
//         hosting_unit_id = HostingUnitIdWebById(_hosting_id=__hosting_id__, project_id=raw_project['oshwaUid'])
//         fetch_result = self.__fetch_one(hosting_unit_id, raw_project, last_visited)
//         log.debug("yield fetch_result %s", hosting_unit_id)
//         yield fetch_result

//     # save current progress
//     batch_size = data["limit"]  # in case the batch size will be lowered on the platform in some point in time
//     num_fetched += len(data["items"])
//     last_offset += batch_size
//     if last_offset > data["total"]:
//         break

//     self._state_repository.store(__hosting_id__, {
//         "last_offset": last_offset,
//         "num_fetched": num_fetched,
//     })
