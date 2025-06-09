// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{
    create_downloader_retry, Config as IConfig, CreationError, Error, Factory as IScraperFactory,
    RetryConfig, Scraper as IScraper, TypeInfo,
};
use crate::model::hosting_unit_id::{HostingUnitId, HostingUnitIdForge, HostingUnitIdWebById};
use crate::{
    files_finder,
    model::{hosting_provider_id::HostingProviderId, hosting_type::HostingType, project::Project},
    settings::PartialSettings,
    tools,
};
use async_std::fs::{self, File};
use async_std::path::{Path, PathBuf};
use async_std::prelude::*;
use async_stream::stream;
use async_trait::async_trait;
use asyncgit::AsyncGitNotification;
use asyncgit::{sync::RepoPath, AsyncPull, FetchRequest};
use crossbeam_channel::unbounded;
use futures::{stream::BoxStream, stream::StreamExt};
use git2::{build::RepoBuilder, FetchOptions, Repository};
use regex::Regex;
use reqwest::header::{HeaderMap, ACCEPT};
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use serde_json::Value;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::LazyLock;
use tracing::instrument;

mod model;

pub static SCRAPER_TYPE: LazyLock<TypeInfo> = LazyLock::new(|| TypeInfo {
    name: "manifests-list",
    description: "Fetches a single JSON file,
containing a list of URLs to manifests and a list of lists.
This is a recursive system.",
    hosting_type: HostingType::ManifestsRepo,
});

#[derive(Deserialize, Debug)]
pub struct Config {
    /// The fetch URL of the root list file (JSON),
    /// which contains a list of links to manifest files
    /// and a list of files following the same format.
    ///
    /// This is a recursive system.
    list_url: String,
    retries: Option<u32>,
    timeout: Option<u64>,
}

impl IConfig for Config {
    fn hosting_provider(&self) -> HostingProviderId {
        HostingProviderId::Inapplicable
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

    // #[instrument]
    async fn scrape(&self) -> BoxStream<'static, Result<Project, Error>> {
        let list_url = self.config.list_url.clone();
        let cache_root_dir = self.generate_cache_root();

        let client = Arc::clone(&self.downloader);
        stream! {
            let mut manifest_urls = vec![];
            tracing::info!("Fetching manifests recursively from remote (root) list '{list_url}' ...");
            Self::fetch_list_recursive(
                Arc::clone(&client),
                &cache_root_dir,
                &list_url,
                &mut manifest_urls,
            )
            .await?;
            tracing::info!(
                "Fetched {} manifests from remote (root) list '{list_url}' ...",
                manifest_urls.len()
            );

            let manifest_urls_file = cache_root_dir.join("manifest-urls.csv");
            tracing::warn!(
                "Writing accumulated manifest URLs to file '{}' ...",
                manifest_urls_file.display()
            );
            let mut manifest_urls_file_handle = File::create(manifest_urls_file)
                .await
                .expect("Unable to create file");
            for manifest_url in &manifest_urls {
                manifest_urls_file_handle
                    .write_all(manifest_url.as_bytes())
                    .await
                    .expect("Unable to write data to manifest-list CSV result file");
                manifest_urls_file_handle
                    .write(b"\n")
                    .await
                    .expect("Unable to write data to manifest-list CSV result file");
            }
            tracing::info!("done.");

            // tracing::info!(
            //     "Fetching the actual manifests ...",
            // );
            let manifest_urls: Vec<String> = vec![]; // TODO FIXME HACK This ensures the following is a no-op, which we have to change, later on
            // tracing::debug!("Parsing {} manifest files found in local repo ...", manifest_file_paths.len());
            for manifest_url in manifest_urls {
                // tracing::debug!("Fetching manifest from URL '{manifest_url}' ...");
                let project_id = HostingUnitId::WebById(HostingUnitIdWebById::from_url(&manifest_url).unwrap().0);
                let project = Project::new(project_id);
                // todo!("Parse manifests and create and yield resulting projects"); // TODO FIXME
                yield Ok(project);
            }
        }.boxed()
    }
}

use futures::future::{BoxFuture, FutureExt};

impl Scraper {
    /// Fetches all manifest URLs from a list file; recursively.
    fn fetch_list_recursive<'params>(
        client: Arc<ClientWithMiddleware>,
        cache_root_dir: &'params Path,
        list_url: &'params str,
        manifest_urls: &'params mut Vec<String>,
    ) -> BoxFuture<'params, Result<(), Error>> {
        async move {
            let list_cache_dir = generate_cache_dir(cache_root_dir, CacheType::List, list_url);
            let list_flat =
                Self::fetch_list_flat(Arc::clone(&client), &list_cache_dir, list_url).await?;
            // tracing::info!("Fetching manifests recursively from remote (root) list '{list_url}' ...");
            match list_flat {
                model::ListFileContent::Simple(manifests) => {
                    manifest_urls.extend(manifests.0);
                }
                model::ListFileContent::V1(list_v1_flat) => {
                    if let Some(manifests) = list_v1_flat.manifests {
                        manifest_urls.extend(manifests);
                    }
                    if let Some(lists) = list_v1_flat.lists {
                        for inner_list_url in lists {
                            Self::fetch_list_recursive(
                                Arc::clone(&client),
                                cache_root_dir,
                                &inner_list_url,
                                manifest_urls,
                            )
                            .await?;
                        }
                    }
                    if let Some(manifest_repos) = list_v1_flat.manifest_repos {
                        todo!("Manifest repos fetching not yet implemented for recursive lists");
                    }
                }
            }
            Ok(())
        }
        .boxed()
    }

    /// Fetches a single list file; non recursively.
    async fn fetch_list_flat(
        client: Arc<ClientWithMiddleware>,
        list_cache_dir: &Path,
        url: &str,
    ) -> Result<model::ListFileContent, Error> {
        if !list_cache_dir.exists().await {
            fs::create_dir_all(&list_cache_dir).await?;
        }
        let content_file = list_cache_dir.join("content.json");
        // let params = [
        //     ("action", "query"),
        //     ("format", "json"),
        //     ("list", "categorymembers"),
        //     ("cmlimit", "max"),
        //     ("cmtitle", "Category:Projects"),
        // ];
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, "application/json".parse().unwrap());
        let content = client
            .get(url)
            .headers(headers)
            // .query(&params)
            .send()
            .await?
            // .json::<model::Content>()
            .text()
            .await?;

        // write to cache
        fs::write(&content_file, &content).await?;

        // parse
        let content_v1_res: Result<model::ListFileContentV1, Error> = Self::parse(&content);
        match content_v1_res {
            Ok(content_v1) => Ok(content_v1.into()),
            Err(err_v1) => {
                let content_simple_res: Result<model::ListFileContentSimple, Error> =
                    Self::parse(&content);
                content_simple_res.map(Into::into)
            }
        }
    }

    // #[instrument]
    fn parse<T: serde::de::DeserializeOwned>(content: &str) -> Result<T, Error> {
        let json_val = serde_json::from_str::<Value>(content)
            .map_err(|json_err| Error::DeserializeAsJson(json_err, content.to_string()))?;

        serde_json::from_value::<T>(json_val)
            .map_err(|serde_err| Error::Deserialize(serde_err, content.to_string()))
    }

    fn generate_cache_root(&self) -> PathBuf {
        self.config_all
            .database
            .path
            .join("_tmp")
            .join(SCRAPER_TYPE.name)
            .into()
    }
}

fn generate_cache_dir(cache_root: &Path, r#type: CacheType, url: &str) -> PathBuf {
    cache_root
        .join(r#type.as_str())
        .join(tools::url_encode(url).as_ref())
}

#[derive(Clone, Copy)]
enum CacheType {
    Manifest,
    List,
    Repo,
}

impl CacheType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manifest => "manifest",
            Self::List => "list",
            Self::Repo => "repo",
        }
    }
}
