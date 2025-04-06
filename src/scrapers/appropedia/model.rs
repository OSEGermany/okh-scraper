// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::super::Error;
use crate::model::hosting_provider_id::HostingProviderId;
use serde::Deserialize;

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
pub(super) struct Projects {
    // #[serde(rename = "batchcomplete")]
    // batch_complete: String,
    limits: Limits,
    query: Query,
}

impl Projects {
    // NOTE Do **NOT** make this `const`! It will fail on CI.
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
