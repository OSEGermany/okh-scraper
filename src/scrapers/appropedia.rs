// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{
    create_downloader_retry, CreationError, Error, Factory as IScraperFactory, PlatformBaseConfig,
    RetryConfig, Scraper as IScraper, TypeInfo,
};
use crate::model::hosting_type::HostingType;
use crate::structured_content::{Chunk, SerializationFormat};
use crate::{
    model::{
        hosting_provider_id::HostingProviderId, hosting_unit_id::HostingUnitId, project::Project,
    },
    settings::PartialSettings,
    structured_content::RawContent,
    tools,
};
use async_stream::stream;
use async_trait::async_trait;
use futures::{stream::BoxStream, stream::StreamExt};
use reqwest::header::{HeaderMap, ACCEPT, USER_AGENT};
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use serde_json::Value;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::LazyLock;

pub const HOSTING_PROVIDER_ID: HostingProviderId = HostingProviderId::AppropediaOrg;

pub static SCRAPER_TYPE: LazyLock<TypeInfo> = LazyLock::new(|| TypeInfo {
    name: "appropedia",
    description: "Fetches projects from Appropedia (appropedia.org),
(<https://appropedia.org/>).
The project names/IDs are fetched through their API,
and then each projects (generated) OKHv1 YAML manifest is downloaded separately.",
    hosting_type: HostingType::Appropedia,
});

pub type Config = PlatformBaseConfig;

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
        let downloader = create_downloader_retry(&config);
        Ok(Box::new(Scraper {
            config_all,
            config,
            downloader,
        }))
    }
}

pub struct Scraper {
    config_all: Arc<PartialSettings>,
    config: Config,
    downloader: Arc<ClientWithMiddleware>,
}

#[async_trait(?Send)]
impl super::Scraper for Scraper {
    fn info(&self) -> &'static TypeInfo {
        &SCRAPER_TYPE
    }

    async fn scrape(&self) -> BoxStream<'static, Result<Project, Error>> {
        let client = Arc::<_>::clone(&self.downloader);
        match self.fetch_project_names(Arc::<_>::clone(&client)).await {
            Ok(project_names) => stream! {
                for project_name in project_names {
                    match Self::fetch_okhv1_manifest(Arc::<_>::clone(&client), &project_name).await {
                        Ok(manifest_raw) => {
                            let mut project = Project::new(HostingUnitId::from((HOSTING_PROVIDER_ID, project_name)));
                            project.manifest = Some(Chunk::from_content(SerializationFormat::Yaml, manifest_raw));
                            yield Ok(project);
                        },
                        Err(err) => {
                            yield Err(err);
                        }
                    }
                }
            }.boxed(),
            Err(err) => stream! {
                yield Err(err);
            }.boxed(),
        }
    }
}

impl Scraper {
    async fn fetch_project_names(
        &self,
        // client: Arc<Box<Client>>,
        client: Arc<ClientWithMiddleware>,
    ) -> Result<Vec<String>, Error> {
        let params = [
            ("action", "query"),
            ("format", "json"),
            ("list", "categorymembers"),
            ("cmlimit", "max"),
            ("cmtitle", "Category:Projects"),
        ];
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, "application/json".parse().unwrap());
        headers.insert(
            USER_AGENT,
            "okh-scraper github.com/iop-alliance/OpenKnowHow"
                .parse()
                .unwrap(),
        );
        // let client = reqwest::Client::new();
        let res = client
            .get("https://www.appropedia.org/w/api.php")
            .headers(headers)
            .query(&params)
            .send()
            .await?
            .json::<Projects>()
            // .text()
            .await?;
        res.check_limit(self.config.hosting_provider)?;
        let project_titles: Vec<String> = res.into();
        tracing::debug!("{:#?}", project_titles);
        Ok(project_titles)
    }

    async fn fetch_okhv1_manifest(
        // client: Arc<Box<ClientWithMiddleware>>,
        client: Arc<ClientWithMiddleware>,
        project_title: &str,
    ) -> Result<RawContent, Error> {
        let project_title_no_spaces = project_title.replace(' ', "_");
        let project_title_encoded = tools::url_encode(&project_title_no_spaces);
        let manifest_dl_url = format!("https://www.appropedia.org/scripts/generateOpenKnowHowManifest.php?title={project_title_encoded}");

        // let client = reqwest::Client::new();
        let res = client
            .get(manifest_dl_url)
            .send()
            .await?
            // .json::<ProjectNames>()
            .text()
            .await?;
        tracing::debug!("{res:#?}");
        Ok(res.into())
    }
}

#[derive(Deserialize, Debug)]
struct Limits {
    #[serde(rename = "categorymembers")]
    category_members: usize,
}

#[derive(Deserialize, Debug)]
struct Page {
    // #[serde(rename = "pageid")]
    // id: usize,
    // ns: usize,
    title: String,
}

#[derive(Deserialize, Debug)]
struct Query {
    #[serde(rename = "categorymembers")]
    category_members: Vec<Page>,
}

#[derive(Deserialize, Debug)]
struct Projects {
    // #[serde(rename = "batchcomplete")]
    // batch_complete: String,
    limits: Limits,
    query: Query,
}

impl Projects {
    pub fn check_limit(&self, hosting_provider: HostingProviderId) -> Result<(), Error> {
        if self.query.category_members.len() == self.limits.category_members {
            return Err(Error::FetchLimitReached(
                hosting_provider,
                self.limits.category_members,
            ));
        }
        Ok(())
    }
}

impl From<Projects> for Vec<String> {
    fn from(value: Projects) -> Self {
        value
            .query
            .category_members
            .iter()
            .map(|p| p.title.clone())
            .collect()
    }
}
