// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::scrapers::manifests_repo;

use super::hosting_category;
use hosting_category::HostingCategory;
use std::fmt;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy)]
pub enum NetworkTopology {
    /// No network is involved.
    /// This might be the case e.g. with local files
    /// or fully generated data.
    NotNetworked,
    Centralized,
    /// Aka Federated
    Decentralized,
    Distributed,
}

/// Descriptive data about the hosting technology,
/// be it e.g. a Platform like GitHub.com,
/// or rather a technology like IPFS.
pub struct HostingTypeInfo {
    /// Human-readable name/id of this type of hosting technology.
    pub name: &'static str,
    /// Web-URL with info about this type of hosting technology.
    pub url: &'static str,
    /// How is the network part of this hosting technology
    /// fundamentally structured.
    pub network_topology: NetworkTopology,
}

pub static HTI_APPROPEDIA: LazyLock<HostingTypeInfo> = LazyLock::new(|| HostingTypeInfo {
    name: "Appropedia OSH Wiki",
    url: "https://appropedia.org/",
    network_topology: NetworkTopology::Centralized,
});

pub static HTI_MANIFESTS_REPO: LazyLock<HostingTypeInfo> = LazyLock::new(|| HostingTypeInfo {
    name: "Git repositories containing manifests",
    url: "https://git-scm.com",
    network_topology: NetworkTopology::Decentralized,
});

pub static HTI_OSHWA: LazyLock<HostingTypeInfo> = LazyLock::new(|| HostingTypeInfo {
    name: "OSHWA OSH Certification Platform",
    url: "https://certification.oshwa.org/",
    network_topology: NetworkTopology::Centralized,
});

/// Type of hosting technology,
/// meaning the software used to host,
/// in case of (de)centralized systems,
/// or the technology itself,
/// in case of distributed systems.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostingType {
    Appropedia,
    ForgeJo,
    GitHub,
    GitLab,
    Oshwa,
    Thingiverse,
    ManifestsRepo,
}

impl AsRef<str> for HostingType {
    fn as_ref(&self) -> &str {
        match self {
            Self::Appropedia => "SW|appropedia.org",
            Self::ForgeJo => "SW|forgejo.org",
            Self::GitHub => "SW|github.com",
            Self::GitLab => "SW|gitlab.com",
            Self::Oshwa => "SW|oshwa.org",
            Self::Thingiverse => "SW|thingiverse.com",
            Self::ManifestsRepo => "SW|git-scm.com", // TODO Maybe write a tiny documentation about this, and then link to that?
        }
    }
}

impl fmt::Display for HostingType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl HostingType {
    #[must_use]
    pub const fn category(&self) -> HostingCategory {
        match self {
            Self::ForgeJo | Self::GitHub | Self::GitLab => HostingCategory::Forge,
            Self::Appropedia | Self::Oshwa | Self::Thingiverse | Self::ManifestsRepo => {
                HostingCategory::Other
            }
        }
    }

    #[must_use]
    pub fn info(&self) -> &'static HostingTypeInfo {
        match self {
            Self::ForgeJo => todo!("Implement"),
            Self::GitHub => todo!("Implement"),
            Self::GitLab => todo!("Implement"),
            Self::Appropedia => &HTI_APPROPEDIA,
            Self::Oshwa => &HTI_OSHWA,
            Self::Thingiverse => todo!("Implement"),
            Self::ManifestsRepo => &HTI_MANIFESTS_REPO,
        }
    }
}
