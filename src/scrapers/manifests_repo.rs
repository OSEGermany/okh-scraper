// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{
    create_downloader_retry, Config as IConfig, CreationError, Error, Factory as IScraperFactory,
    RetryConfig, Scraper as IScraper, TypeInfo,
};
use crate::model::hosting_unit_id::{
    HostingUnitId, HostingUnitIdForge, HostingUnitIdManifestInRepo,
};
use crate::{
    files_finder,
    model::{hosting_provider_id::HostingProviderId, hosting_type::HostingType, project::Project},
    settings::PartialSettings,
    tools,
};
use async_stream::stream;
use async_trait::async_trait;
use asyncgit::AsyncGitNotification;
use asyncgit::{sync::RepoPath, AsyncPull, FetchRequest};
use crossbeam_channel::unbounded;
use futures::{stream::BoxStream, stream::StreamExt};
use git2::{build::RepoBuilder, FetchOptions, Repository};
use regex::Regex;
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use serde_json::Value;
use std::sync::LazyLock;
use std::{path::PathBuf, sync::Arc};
use tracing::instrument;

pub static SCRAPER_TYPE: LazyLock<TypeInfo> = LazyLock::new(|| TypeInfo {
    name: "manifests-repo",
    description: "Fetches a single git repository,
and then scans it for manifest files.",
    hosting_type: HostingType::ManifestsRepo,
});

pub static RE_MANIFEST_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.(toml|ya?ml|json)$").unwrap());
// LazyLock::new(|| Regex::new(r"^(?i)\.(toml$").unwrap());

#[derive(Deserialize, Debug)]
pub struct Config {
    /// The git Fetch URL of the repository
    /// that contains manifest files.
    /// As of now, this has to be a public, anonymous access URL.
    repo_fetch_url: String,
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
        config_fetcher: Value,
    ) -> Result<Box<dyn IScraper>, CreationError> {
        let config: Config = serde_json::from_value(config_fetcher).unwrap();
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
    async fn fetch_all(&self) -> BoxStream<'static, Result<Project, Error>> {
        let fetch_url = self.config.repo_fetch_url.clone();
        let repo_local_dir = self.generate_repo_local_dir(&fetch_url);
        tracing::info!(
            "Fetching manifests containing remote repo '{fetch_url}' to local dir '{}' ...",
            repo_local_dir.display()
        );
        if repo_local_dir.exists() {
            tracing::trace!(
                "Pulling remote repo '{fetch_url}' to local dir '{}' ...",
                repo_local_dir.display()
            );
            let repo_path = RepoPath::Path(repo_local_dir.clone());
            let (s1, r) = unbounded();
            let pull = AsyncPull::new(repo_path, &s1);
            if let Err(err) = pull.request(FetchRequest::default()) {
                return stream! {
                    yield Err(err.into());
                }
                .boxed();
            }

            // For some reason. we get two pull notifications
            for notification_idx in 0..2 {
                match r.recv() {
                    Ok(AsyncGitNotification::Pull) => {
                        tracing::trace!("Received pull notification {notification_idx}/2.");
                    }
                    Ok(notification) => {
                        tracing::error!(
                            "Something went wrong while pulling '{fetch_url}': '{notification:#?}'"
                        );
                        return stream! {
                            yield Err(Error::FailedGit(format!("Something went wrong while pulling '{fetch_url}': '{notification:#?}'")));
                        }.boxed();
                    }
                    Err(err) => {
                        tracing::error!("Error while pulling '{fetch_url}': {err:?}");
                        return stream! {
                            yield Err(Error::FailedGit(format!("Error while pulling '{fetch_url}': {err:?}")));
                        }.boxed();
                    }
                }
            }
        } else {
            tracing::trace!(
                "Cloning remote repo '{fetch_url}' to local dir '{}' ...",
                repo_local_dir.display()
            );
            let mut fetch_options = FetchOptions::new();
            fetch_options.depth(1);
            let repo_clone_res = RepoBuilder::new()
                .fetch_options(fetch_options)
                .clone(&fetch_url, repo_local_dir.as_ref())
                .map_err(Error::from)
                .map(|_| ());

            if let Err(err) = repo_clone_res {
                tracing::error!("Error while pulling '{fetch_url}': {err:?}");
                return stream! {
                    yield Err(err.into());
                }
                .boxed();
            }
            tracing::trace!(
                "Cloning remote repo '{fetch_url}' to local dir '{}' - done.",
                repo_local_dir.display()
            );
        };
        tracing::trace!(
            "Searching manifests in local dir '{}' ...",
            repo_local_dir.display()
        );
        stream! {
            let (repo, repo_internal_base_path) = HostingUnitIdForge::from_url(&fetch_url)?;
            let local_base_path = repo_local_dir.join(repo_internal_base_path.unwrap_or_default());
            // TODO This (finding files recursively by regex) could be made async too:
            // git ls-tree --full-tree -r --name-only HEAD
            tracing::debug!("Searching manifest files in local repo: '{}' ...", local_base_path.display());
            let manifest_file_paths = files_finder::find_recursive(local_base_path, &RE_MANIFEST_FILE)?;
            tracing::debug!("Parsing {} manifest files found in local repo ...", manifest_file_paths.len());
            for path in manifest_file_paths {
                tracing::debug!("Parsing manifest file '{}' ...", path.display());
                let project_id = HostingUnitId::ManifestInRepo(HostingUnitIdManifestInRepo::new(repo.clone(), path));
                let project = Project::new(project_id);
                // todo!("Parse manifests and create and yield resulting projects"); // TODO FIXME
                yield Ok(project);
            }
        }.boxed()
    }
}

impl Scraper {
    fn generate_repo_local_dir(&self, url: &str) -> PathBuf {
        self.config_all
            .database
            .path
            .join("_tmp")
            .join(SCRAPER_TYPE.name)
            .join(tools::url_encode(url).as_ref())
    }

    // async fn fetch_project_names() -> BoxResult<Vec<String>> {
    //     let params = [
    //         ("action", "query"),
    //         ("format", "json"),
    //         ("list", "categorymembers"),
    //         ("cmlimit", "max"),
    //         ("cmtitle", "Category:Projects"),
    //     ];
    //     let mut headers = HeaderMap::new();
    //     headers.insert(ACCEPT, "application/json".parse().unwrap());
    //     headers.insert(
    //         USER_AGENT,
    //         "okh-scraper github.com/iop-alliance/OpenKnowHow"
    //             .parse()
    //             .unwrap(),
    //     );
    //     let client = reqwest::Client::new();
    //     let res = client
    //         .get("https://www.appropedia.org/w/api.php")
    //         .headers(headers)
    //         .query(&params)
    //         .send()
    //         .await?
    //         .json::<Projects>()
    //         // .text()
    //         .await?;
    //     res.check_limit()?;
    //     let project_titles: Vec<String> = res.into();
    //     tracing::debug!("{:#?}", project_titles);
    //     Ok(project_titles)
    // }

    // async fn fetch_okhv1_manifest(project_title: &str) -> BoxResult<String> {
    //     let project_title_no_spaces = project_title.replace(" ", "_");
    //     let project_title_encoded = tools::url_encode(&project_title_no_spaces);
    //     let manifest_dl_url = format!("https://www.appropedia.org/scripts/generateOpenKnowHowManifest.php?title={project_title_encoded}");

    //     let client = reqwest::Client::new();
    //     let res = client
    //         .get(manifest_dl_url)
    //         .send()
    //         .await?
    //         // .json::<ProjectNames>()
    //         .text()
    //         .await?;
    //     tracing::debug!("{res:#?}");
    //     Ok(res)
    // }
}
