// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::hosting_provider_id::{self, HostingProviderId};
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::task::id;
use url;
use url::Url;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Failed to parse input as URL at all: '{0}'")]
    InvalidUrl(#[from] url::ParseError),
    #[error("Invalid URL structure for a project '{0}'")]
    InvalidStructure(String),
    #[error("Bad amount of path parts for a {1} project URL '{0}' - should be {2}, but is {3}")]
    InvalidNumOfPathParts(String, HostingProviderId, usize, usize),
    #[error(
        "Bad amount of path parts for a {1} project URL '{0}' - should be at least {2}, but is {3}"
    )]
    InsufficientNumOfPathParts(String, HostingProviderId, usize, usize),
    #[error("Bad format for a {1} project URL '{0}' - {2}")]
    InvalidFormat(String, HostingProviderId, String),
    #[error("Failed to parse hosting-provider: '{0}'")]
    HostingProviderParseError(#[from] hosting_provider_id::ParseError),
    #[error("The URL '{0}' is not a valid {1} hosting provider URL")]
    HostingProviderNotSupported(String, HostingProviderId),
    #[error("The URL '{0}' indicates the unsupported hosting provider {1}")]
    UnsupportedHostingProvider(String, HostingProviderId),
    #[error("Failed to validate hosting-unit-id")]
    ValidationFailed(HostingUnitId),
    #[error("Missing host part in project URL '{0}'")]
    MissingHost(String),
}

#[derive(Debug, Error)]
pub enum SerializeError {
    #[error("The hosting provider {0} does not support downloading individual files")]
    DownloadNotSupported(HostingProviderId),
    #[error("Unsupported hosting-provider")]
    UnsupportedHostingProvider(HostingProviderId),
    #[error("Failed to parse generated URL: '{0}'")]
    UrlParseError(#[from] url::ParseError),
}

/// This identifies the hosting location of a single project.
///
/// In the case of a centralized or decentralized hosting technology,
/// This simply identifies that platform
/// (or its specific hosting, if decentralized),
/// plus the project ID
/// (possibly combined out of several properties,
/// like owner and repo-name).
///
/// In the case of a distributed hosting technology like IPFS
/// or repositories which each contains multiple manifest-files,
/// This is the location of the manifest-file,
/// however that may be made up,
/// e.g. the hosting-technology +
/// the DHT-hash +
/// (optional) the manifest path within.
#[derive(Debug, PartialEq, Eq)]
pub enum HostingUnitId {
    Forge(HostingUnitIdForge),
    WebById(HostingUnitIdWebById),
    ManifestInRepo(HostingUnitIdManifestInRepo),
}

impl From<(HostingProviderId, String)> for HostingUnitId {
    fn from(value: (HostingProviderId, String)) -> Self {
        if matches!(value.0, HostingProviderId::Inapplicable) {
            panic!("Code-logic should never allow to get here");
        }
        Self::WebById(HostingUnitIdWebById {
            hosting_provider: value.0,
            project_id: value.1,
        })
    }
}

impl From<(HostingProviderId, String, String)> for HostingUnitId {
    fn from(value: (HostingProviderId, String, String)) -> Self {
        Self::Forge(HostingUnitIdForge {
            hosting_provider: value.0,
            owner: value.1,
            repo: value.2,
            group_hierarchy: None,
            reference: None,
        })
    }
}

trait IHostingUnitId: PartialEq + Eq /* + fmt::Display*/ {
    fn to_path_str(&self) -> String;
    fn hosting_provider(&self) -> HostingProviderId;
    fn references_version(&self) -> bool;
    fn is_valid(&self) -> bool;
    fn create_project_hosting_url(&self) -> Result<Url, SerializeError>;
    fn create_download_url(&self, path: &Path) -> Result<Url, SerializeError>;
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct HostingUnitIdForge {
    hosting_provider: HostingProviderId,
    owner: String,
    repo: String,
    group_hierarchy: Option<String>,
    reference: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct HostingUnitIdWebById {
    hosting_provider: HostingProviderId,
    project_id: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct HostingUnitIdManifestInRepo {
    repo: HostingUnitIdForge,
    /// Repo-root relative path to the manifest file within the repo.
    path: PathBuf,
}

impl HostingUnitIdForge {
    pub fn from_url(url: &str) -> Result<(Self, Option<PathBuf>), ParseError> {
        let hosting_provider = HostingProviderId::from_url(url)?;
        let parsed_url = Url::parse(url)?;
        let host = parsed_url
            .host()
            .ok_or_else(|| ParseError::MissingHost(url.to_string()))?
            .to_string()
            .to_lowercase();
        // let domain = parsed_url.host().unwrap().to_string().to_lowercase();
        let path_parts: Vec<_> = Path::new(parsed_url.path())
            .strip_prefix("/")
            .map_err(|_err| ParseError::InvalidStructure(url.to_string()))?
            .iter()
            .collect();

        let (owner, repo, group_hierarchy, reference, path) = match hosting_provider {
            HostingProviderId::GitHubCom | HostingProviderId::CodebergOrg => {
                if path_parts.len() < 2 {
                    return Err(ParseError::InsufficientNumOfPathParts(
                        url.to_string(),
                        hosting_provider,
                        2,
                        path_parts.len(),
                    ));
                }
                let owner = path_parts[0].to_str().unwrap().to_string();
                let repo = path_parts[1].to_str().unwrap().to_string();
                let (reference, path) = if host == "raw.githubusercontent.com" {
                    (
                        path_parts.get(2).map(|p| p.to_str().unwrap().to_string()),
                        path_parts.get(3).map(PathBuf::from),
                    )
                } else {
                    let mut reference = None;
                    let mut path = None;
                    if path_parts.len() >= 4
                        && ["tree", "blob", "raw"].contains(&path_parts[2].to_str().unwrap())
                    {
                        reference = Some(path_parts[3].to_str().unwrap().to_string());
                        if path_parts.len() > 4 {
                            let mut path_buf = PathBuf::from("/");
                            for p in &path_parts[4..] {
                                path_buf.push(p);
                            }
                            path = Some(path_buf);
                        }
                    } else if path_parts.len() > 4
                        && path_parts[2] == "releases"
                        && path_parts[3] == "tag"
                    {
                        reference = Some(path_parts[4].to_str().unwrap().to_string());
                    } else if path_parts.len() > 3 && path_parts[2] == "commit" {
                        reference = Some(path_parts[3].to_str().unwrap().to_string());
                    }
                    // else {
                    //     return Err(ParseError::InsufficientNumOfPathParts(
                    //         url.to_string(),
                    //         hosting_provider,
                    //         4,
                    //         path_parts.len(),
                    //     ));
                    // }
                    (reference, path)
                };
                (owner, repo, None, reference, path)
            }
            HostingProviderId::GitLabCom => {
                if path_parts.len() < 2 {
                    return Err(ParseError::InsufficientNumOfPathParts(
                        url.to_string(),
                        hosting_provider,
                        2,
                        path_parts.len(),
                    ));
                }
                let owner = path_parts[0].to_str().unwrap().to_string();
                let repo = path_parts[1].to_str().unwrap().to_string();
                let (reference, path) = if path_parts.len() >= 5
                    && path_parts[2] == "-"
                    && ["tree", "blob", "raw"].contains(&path_parts[3].to_str().unwrap())
                {
                    (
                        Some(path_parts[4].to_str().unwrap().to_string()),
                        path_parts.get(5).map(Path::new),
                    )
                } else if path_parts.len() >= 5
                    && path_parts[2] == "-"
                    && ["commit", "tags"].contains(&path_parts[3].to_str().unwrap())
                {
                    (Some(path_parts[4].to_str().unwrap().to_string()), None)
                } else {
                    (None, None)
                };
                (owner, repo, None, reference, path.map(PathBuf::from))
            }
            HostingProviderId::AppropediaOrg
            | HostingProviderId::OshwaOrg
            | HostingProviderId::ThingiverseCom => {
                return Err(ParseError::HostingProviderNotSupported(
                    url.to_string(),
                    hosting_provider,
                ));
            }
            _ => {
                return Err(ParseError::UnsupportedHostingProvider(
                    url.to_string(),
                    hosting_provider,
                ));
            }
        };

        let hosting_unit_id = Self {
            hosting_provider,
            owner,
            repo,
            group_hierarchy,
            reference,
        };

        Ok((hosting_unit_id, path))
    }
}

impl Display for HostingUnitId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Forge(ref id) => f.write_str(&id.to_path_str()),
            Self::WebById(ref id) => f.write_str(&id.to_path_str()),
            Self::ManifestInRepo(ref id) => f.write_str(&id.to_path_str()),
        }
    }
}

// impl Display for HostingUnitIdForge {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.write_str(&self.to_path_str())
//     }
// }
// impl<T> Display for T
// where
// T: IHostingUnitId
// {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.write_str(&self.to_path_str())
//     }
// }

impl IHostingUnitId for HostingUnitIdForge {
    fn to_path_str(&self) -> String {
        format!(
            "{}/{}/{}{}{}",
            self.hosting_provider,
            self.owner,
            self.group_hierarchy.as_ref().map_or("", |x| x),
            self.repo,
            self.reference.as_ref().map_or("", |x| x),
        )
    }

    fn hosting_provider(&self) -> HostingProviderId {
        self.hosting_provider
    }

    fn references_version(&self) -> bool {
        self.reference.is_some()
    }

    fn is_valid(&self) -> bool {
        [
            HostingProviderId::CodebergOrg,
            HostingProviderId::GitHubCom,
            HostingProviderId::GitLabCom,
        ]
        .contains(&self.hosting_provider)
    }

    fn create_project_hosting_url(&self) -> Result<Url, SerializeError> {
        let url_str = match self.hosting_provider {
            HostingProviderId::GitHubCom => {
                format!("https://github.com/{}/{}/", self.owner, self.repo)
            }
            HostingProviderId::CodebergOrg => {
                format!("https://codeberg.org/{}/{}/", self.owner, self.repo)
            }
            HostingProviderId::GitLabCom => {
                format!("https://gitlab.com/{}/{}/", self.owner, self.repo)
            }
            HostingProviderId::Inapplicable
            | HostingProviderId::AppropediaOrg
            | HostingProviderId::OshwaOrg
            | HostingProviderId::ThingiverseCom => {
                return Err(SerializeError::UnsupportedHostingProvider(
                    self.hosting_provider,
                ))
            }
        };

        Url::parse(&url_str).map_err(SerializeError::UrlParseError)
    }

    fn create_download_url(&self, path: &Path) -> Result<Url, SerializeError> {
        let url_str = match self.hosting_provider {
            HostingProviderId::GitHubCom => format!(
                "https://raw.githubusercontent.com/{}/{}/{}",
                self.owner,
                self.repo,
                path.display()
            ),
            HostingProviderId::CodebergOrg => format!(
                "https://codeberg.org/{}/{}/raw/{}",
                self.owner,
                self.repo,
                path.display()
            ),
            HostingProviderId::GitLabCom => format!(
                "https://gitlab.com/{}/{}/-/raw/{}",
                self.owner,
                self.repo,
                path.display()
            ),
            HostingProviderId::Inapplicable
            | HostingProviderId::AppropediaOrg
            | HostingProviderId::OshwaOrg
            | HostingProviderId::ThingiverseCom => {
                return Err(SerializeError::UnsupportedHostingProvider(
                    self.hosting_provider,
                ))
            }
        };

        Url::parse(&url_str).map_err(SerializeError::UrlParseError)
    }
}

// impl Display for HostingUnitIdWebById {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.write_str(&self.to_path_str())
//     }
// }

impl HostingUnitIdWebById {
    pub fn from_url(url: &str) -> Result<(Self, Option<PathBuf>), ParseError> {
        let hosting_provider = HostingProviderId::from_url(url)?;
        let parsed_url = url::Url::parse(url).map_err(ParseError::InvalidUrl)?;
        let path_parts: Vec<_> = Path::new(parsed_url.path())
            .strip_prefix("/")
            .map_err(|_err| ParseError::InvalidStructure(url.to_string()))?
            .iter()
            .collect();

        let (project_id, path) = match hosting_provider {
            HostingProviderId::Inapplicable => {
                return Err(ParseError::UnsupportedHostingProvider(
                    url.to_string(),
                    hosting_provider,
                ));
            }
            HostingProviderId::GitHubCom
            | HostingProviderId::CodebergOrg
            | HostingProviderId::GitLabCom => {
                return Err(ParseError::InvalidStructure(url.to_string()));
            }
            HostingProviderId::AppropediaOrg => {
                if path_parts.len() > 1 {
                    return Err(ParseError::InvalidNumOfPathParts(
                        url.to_string(),
                        hosting_provider,
                        1,
                        path_parts.len(),
                    ));
                }
                (path_parts[0].to_str().unwrap().to_string(), None)
            }
            HostingProviderId::OshwaOrg => {
                if path_parts.is_empty() {
                    return Err(ParseError::InvalidNumOfPathParts(
                        url.to_string(),
                        hosting_provider,
                        1,
                        path_parts.len(),
                    ));
                }
                (
                    path_parts[0]
                        .to_str()
                        .unwrap()
                        .to_string()
                        .replace(".html", ""),
                    None,
                )
            }
            HostingProviderId::ThingiverseCom => {
                if path_parts.is_empty() {
                    return Err(ParseError::InsufficientNumOfPathParts(
                        url.to_string(),
                        hosting_provider,
                        1,
                        path_parts.len(),
                    ));
                }
                let id_parts = path_parts[0]
                    .to_str()
                    .unwrap()
                    .split(':')
                    .collect::<Vec<_>>();
                if id_parts.len() < 2 || id_parts[0] != "thing" {
                    return Err(ParseError::InvalidFormat(
                        url.to_string(),
                        hosting_provider,
                        "Not a thing URL".to_string(),
                    ));
                }
                (id_parts[1].to_string(), None)
            }
        };

        let hosting_unit_id = Self {
            hosting_provider,
            project_id,
        };

        if !hosting_unit_id.is_valid() {
            return Err(ParseError::ValidationFailed(HostingUnitId::WebById(
                hosting_unit_id,
            )));
        }
        Ok((hosting_unit_id, path))
    }

    #[must_use]
    pub const fn from_parts(hosting_provider: HostingProviderId, project_id: String) -> Self {
        Self {
            hosting_provider,
            project_id,
        }
    }
}

impl IHostingUnitId for HostingUnitIdWebById {
    fn to_path_str(&self) -> String {
        format!("{}/{}", self.hosting_provider, self.project_id)
    }

    fn hosting_provider(&self) -> HostingProviderId {
        self.hosting_provider
    }

    fn references_version(&self) -> bool {
        false
    }

    fn is_valid(&self) -> bool {
        [
            HostingProviderId::AppropediaOrg,
            HostingProviderId::OshwaOrg,
            HostingProviderId::ThingiverseCom,
        ]
        .contains(&self.hosting_provider)
    }

    fn create_project_hosting_url(&self) -> Result<Url, SerializeError> {
        let (url_domain, url_path) = match self.hosting_provider {
            HostingProviderId::AppropediaOrg => {
                ("www.appropedia.org", format!("/{}", self.project_id))
            }
            HostingProviderId::OshwaOrg => (
                "certification.oshwa.org",
                format!("/{}.html", self.project_id.to_lowercase()),
            ),
            HostingProviderId::ThingiverseCom => {
                ("www.thingiverse.com", format!("/thing:{}", self.project_id))
            }
            HostingProviderId::Inapplicable
            | HostingProviderId::CodebergOrg
            | HostingProviderId::GitHubCom
            | HostingProviderId::GitLabCom => {
                return Err(SerializeError::UnsupportedHostingProvider(
                    self.hosting_provider,
                ))
            }
        };

        let url_str = format!("https://{url_domain}{url_path}");
        Url::parse(&url_str).map_err(SerializeError::UrlParseError)
    }

    fn create_download_url(&self, path: &Path) -> Result<Url, SerializeError> {
        match self.hosting_provider {
            HostingProviderId::AppropediaOrg | HostingProviderId::OshwaOrg => {
                Err(SerializeError::DownloadNotSupported(self.hosting_provider))
            }
            HostingProviderId::ThingiverseCom => {
                // NOTE This _might_ work, if the `path` is indeed a download location
                let url_str = format!(
                    "{}/{}",
                    self.create_project_hosting_url()?,
                    path.to_str().unwrap()
                );
                Url::parse(&url_str).map_err(SerializeError::UrlParseError)
            }
            HostingProviderId::Inapplicable
            | HostingProviderId::CodebergOrg
            | HostingProviderId::GitHubCom
            | HostingProviderId::GitLabCom => Err(SerializeError::UnsupportedHostingProvider(
                self.hosting_provider,
            )),
        }
    }
}

// impl Display for HostingUnitIdManifestInRepo {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.write_str(&self.to_path_str())
//     }
// }

// type Result2<T, E> = Result<Result<T, E>, E>;
// #[derive(Debug)]
// struct Result2<T, E>(Result<Result<T, E>, E>);

// impl<T, E> From<Result<Result<T, E>, E>> for Result2<T, E> {
//     fn from(value: Result<Result<T, E>, E>) -> Self {
//         Self(value)
//     }
// }

// impl<T, E> Result2<T, E> {
//     pub fn flatten(self) -> Result<T, E> {
//         self.0.unwrap()
//     }
// }

impl HostingUnitIdManifestInRepo {
    #[must_use]
    pub const fn new(repo: HostingUnitIdForge, path: PathBuf) -> Self {
        Self { repo, path }
    }
}

impl IHostingUnitId for HostingUnitIdManifestInRepo {
    fn to_path_str(&self) -> String {
        self.path.display().to_string()
    }

    fn hosting_provider(&self) -> HostingProviderId {
        HostingProviderId::Inapplicable
    }

    fn references_version(&self) -> bool {
        self.repo.references_version()
    }

    fn is_valid(&self) -> bool {
        self.repo.is_valid() && self.path.is_relative()
    }

    fn create_project_hosting_url(&self) -> Result<Url, SerializeError> {
        self.repo.create_project_hosting_url().map(|url| {
            url.join(&self.path.display().to_string())
                .map_err(SerializeError::from)
        })?
    }

    fn create_download_url(&self, path: &Path) -> Result<Url, SerializeError> {
        todo!()
    }
}
