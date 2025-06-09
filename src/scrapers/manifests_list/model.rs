// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::Deserialize;

pub(super) enum ListFileContent {
    Simple(ListFileContentSimple),
    V1(ListFileContentV1),
}

impl From<ListFileContentSimple> for ListFileContent {
    fn from(value: ListFileContentSimple) -> Self {
        Self::Simple(value)
    }
}

impl From<ListFileContentV1> for ListFileContent {
    fn from(value: ListFileContentV1) -> Self {
        Self::V1(value)
    }
}

/// This is a Serde mapping of a simplistic JSON file structure,
/// containing only a list of manifest URLs.
#[derive(Deserialize, Debug)]
pub(super) struct ListFileContentSimple(pub(super) Vec<String>);

/// This is a direct Serde mapping of Kaspars original JSON file structure.
#[derive(Deserialize, Debug)]
// #[serde(rename_all = "camelCase")]
pub(super) struct ListFileContentV1 {
    /// A list of URLs to download more JSON files having this same format.
    #[serde(rename = "remoteLists")]
    pub(super) lists: Option<Vec<String>>,
    /// A list of URLs to download a manifest file each.
    #[serde(rename = "remoteManifests")]
    pub(super) manifests: Option<Vec<String>>,
    /// A list of Git repositories,
    /// each of which contains manifest files.
    #[serde(rename = "remoteManifestRepos")]
    pub(super) manifest_repos: Option<Vec<Repo>>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(super) struct Repo {
    /// The git fetch URL of the repository
    /// that contains manifest files.
    /// As of now, this has to be a public, anonymous access URL.
    repo_fetch_url: String,
    /// The git ref (branch, tag, commit hash, etc.) to fetch
    r#ref: String,
}
