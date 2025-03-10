// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::hosting_unit_id::HostingUnitId;
use crate::structured_content::Chunk;
use typed_builder::TypedBuilder;

pub type ProjectId = HostingUnitId;
// pub type DataSet = String;
pub type DataSetTree = String;
pub type DataSetRdf = String;
pub type OkhManifest = String;
pub type OkhRdf = String;
pub type OptChunk<T> = Option<Chunk<T>>;

// #[derive(Debug, TypedBuilder)]
#[derive(Debug)]
pub struct Project {
    pub id: ProjectId,
    pub crawling_meta_tree: OptChunk<DataSetTree>,
    pub raw: OptChunk<String>,
    pub manifest: OptChunk<OkhManifest>,
    pub crawling_meta_rdf: OptChunk<DataSetRdf>,
    pub rdf: OptChunk<OkhRdf>,
}

impl Project {
    #[must_use]
    pub const fn new(id: ProjectId) -> Self {
        Self {
            id,
            crawling_meta_tree: None,
            raw: None,
            manifest: None,
            crawling_meta_rdf: None,
            rdf: None,
        }
    }

    #[must_use]
    pub const fn is_whole(&self) -> bool {
        self.crawling_meta_tree.is_some()
            && self.raw.is_some()
            && self.manifest.is_some()
            && self.crawling_meta_rdf.is_some()
            && self.rdf.is_some()
    }
}
