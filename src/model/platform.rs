// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{fs, path::{Path, PathBuf}};
use cli_utils::{BoxError, BoxResult};
use url::Url;
use regex::Regex;
use thiserror::Error;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum HostingId {
    AppropediaOrg,
    CodebergOrg,
    GitHubCom,
    GitLabCom,
    OshwaOrg,
    ThingiverseCom,
}

impl AsRef<str> for HostingId {
    fn as_ref(&self) -> &'static str {
        match self {
            Self::AppropediaOrg => "appropedia.org",
            Self::CodebergOrg => "codeberg.org",
            Self::GitHubCom => "github.com",
            Self::GitLabCom => "gitlab.com",
            Self::ThingiverseCom => "thingiverse.com",
            Self::OshwaOrg => "oshwa.org",
        }
    }
}

impl HostingId {
    fn as_str(&self) -> &'static str {
        self.as_ref()
    }
}

pub struct Platform {
    platform_id: PlatformId,
    owner: String,
    repo: String,
    /// branch, tag, commit hash, etc.
    reference: Option<String>,
}

#[derive(Error, Debug)]
pub enum DownloadUrlGenerationError {
//     Err(format!("This is not a forge(-like) platform: {}", self.platform_id.as_str()))
// _ => Err(format!("Unknown platform '{}'", self.platform_id.as_str())),
    #[error("Unknown platform: '{0}'")]
    UnknownScraperType(String),
    #[error("Missing git reference (branch, tag, commit hash, etc.)")]
    MissingRef,
}

impl Platform {
    fn as_download_url(&self, path: Option<String>) -> Result<String, DownloadUrlGenerationError> {
        match self.platform_id {
            PlatformId::GitHubCom | PlatformId::CodebergOrg => {
                if let Some(reference) = self.reference {
                    let path = match path {
                        Some(path) => format!("/{}/{}/{}{}", self.owner, self.repo, reference, path),
                        None => format!("/{}/{}/{}", self.owner, self.repo, reference),
                    };
                    Ok(format!("https://raw.githubusercontent.com{}", path))
                } else {
                    Err(DownloadUrlGenerationError::MissingRef)
                }
            }
            PlatformId::GitLabCom => {
                if let Some(reference) = self.reference {
                    let path = match path {
                        Some(path) => format!("/{}/{}/-/raw/{}/{}", self.owner, self.repo, reference, path),
                        None => format!("/{}/{}/-/raw/{}", self.owner, self.repo, reference),
                    };
                    Ok(format!("https://gitlab.com{}", path))
                } else {
                    Err(DownloadUrlGenerationError::MissingRef)
                }
            }
            PlatformId::WikiFactoryCom => {
                let sha1_pattern = Regex::new(r"^[0-9a-f]{40}$").unwrap();
                let reference = self.reference.expect("WikiFactory should always have a reference set");
                let path = match (sha1_pattern.is_match(&reference), path) {
                    (true, Some(path)) => format!("/{}/{}/contributions/{}/file/{}", self.owner, self.repo, &reference[..7], path),
                    (true, None) => format!("/{}/{}/contributions/{}/file", self.owner, self.repo, &reference[..7]),
                    (false, Some(path)) => format!("/{}/{}/file/{}", self.owner, self.repo, path),
                    (false, None) => format!("/{}/{}/file", self.owner, self.repo),
                };
                Ok(format!("https://projects.fablabs.io{}", path))
            }
            PlatformId::ThingiverseCom => {
                let path = match path {
                    Some(path) => format!("/{}", path),
                    None => String::new(),
                };
                Ok(format!("https://www.thingiverse.com{}", path))
            }
            PlatformId::AppropediaOrg | PlatformId::OSHWAOrg | PlatformId::ThingiverseCom => {
                Err(format!("This is not a forge(-like) platform: {}", self.platform_id.as_str()))
            }
            _ => Err(format!("Unknown platform '{}'", self.platform_id.as_str())),
        }
    }
}

mod tests {
    use super::*;

    #[test]
    fn test_as_download_url() {
        let platform = Platform {
            platform_id: PlatformId::GitHubCom,
            owner: "owner".to_string(),
            repo: "repo".to_string(),
            reference: "ref".to_string(),
        };
        assert_eq!(platform.as_download_url(None).unwrap(), "https://raw.githubusercontent.com/owner/repo/ref");
        assert_eq!(platform.as_download_url(Some("path".to_string())).unwrap(), "https://raw.githubusercontent.com/owner/repo/ref/path");
    }
}
