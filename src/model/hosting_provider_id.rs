// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::{Deserialize, Serialize};
use std::fmt::Display;
use thiserror::Error;
use url::Url;

use super::hosting_type::HostingType;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Failed to parse as URL: {0}")]
    UrlParseError(#[from] url::ParseError),
    #[error("URL is missing a host part: '{0}'")]
    MissingHost(String),
    #[error("Unrecognized hosting provider host '{1}' in project URL '{0}'")]
    UnrecognizedHost(String, String),
}

/// In case of the hosting being a centralized platform like github.com,
/// this simply identifies that platform.
/// In any other case, [`Self::Inapplicable`] should be used
// / In case of it being a decentralized/multi-instance/self-hostable platform,
// / this identifies a specific one of those instances.
// / In case of a distributed hosting technology,
// / this identifies the network it is part of,
// / if there are multiple ones.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostingProviderId {
    #[default]
    Inapplicable,
    AppropediaOrg,
    CodebergOrg,
    GitHubCom,
    GitLabCom,
    OshwaOrg,
    ThingiverseCom,
    // AManifestsRepo,
}

impl HostingProviderId {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inapplicable => "N/A",
            Self::AppropediaOrg => "appropedia.org",
            Self::CodebergOrg => "codeberg.org",
            Self::GitHubCom => "github.com",
            Self::GitLabCom => "gitlab.com",
            Self::ThingiverseCom => "thingiverse.com",
            Self::OshwaOrg => "oshwa.org",
        }
    }

    pub fn from_url(url: &str) -> Result<Self, ParseError> {
        let parsed_url = Url::parse(url)?;
        let host = parsed_url
            .host()
            .ok_or_else(|| ParseError::MissingHost(url.to_string()))?;
        let host_str = host.to_string().to_lowercase();

        match host_str.as_str() {
            "appropedia.org" | "www.appropedia.org" => Ok(Self::AppropediaOrg),
            "codeberg.org" => Ok(Self::CodebergOrg),
            "github.com" | "raw.githubusercontent.com" => Ok(Self::GitHubCom),
            "gitlab.com" => Ok(Self::GitLabCom),
            "oshwa.org" | "certification.oshwa.org" => Ok(Self::OshwaOrg),
            "thingiverse.com" | "www.thingiverse.com" => Ok(Self::ThingiverseCom),
            _ => Err(ParseError::UnrecognizedHost(url.to_string(), host_str)),
        }
    }

    #[must_use]
    pub const fn r#type(self) -> HostingType {
        match self {
            Self::Inapplicable => HostingType::ManifestsRepo, // TODO FIXME This will likely not hold true in the future
            Self::AppropediaOrg => HostingType::Appropedia,
            Self::CodebergOrg => HostingType::ForgeJo,
            Self::GitHubCom => HostingType::GitHub,
            Self::GitLabCom => HostingType::GitLab,
            Self::ThingiverseCom => HostingType::Thingiverse,
            Self::OshwaOrg => HostingType::Oshwa,
            // Self::ManifestsRepo => HostingType::ManifestsRepo,
        }
    }
}

impl AsRef<str> for HostingProviderId {
    fn as_ref(&self) -> &'static str {
        self.as_str()
    }
}

impl Display for HostingProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
