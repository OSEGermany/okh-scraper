// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{
    thingiverse_store::ThingId, ACPlatformBaseConfig, CreationError, Error,
    Factory as IScraperFactory, Scraper as IScraper, TypeInfo,
};
use crate::{
    model::{
        hosting_provider_id::HostingProviderId,
        hosting_type::HostingType,
        hosting_unit_id::{HostingUnitId, HostingUnitIdWebById},
        project::Project,
    },
    settings::PartialSettings,
    tools::{SpdxLicenseExpression, LICENSE_UNKNOWN},
};
use async_stream::stream;
use async_trait::async_trait;
use futures::{stream::BoxStream, stream::StreamExt};
use governor::{Quota, RateLimiter};
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::sync::LazyLock;
use std::{borrow::Cow, collections::HashMap, fmt::Display, rc::Rc, sync::Arc};
use tokio::time::Duration;
use tracing::instrument;

struct LicenseId {
    pub thingiverse_short: Option<&'static str>,
    pub spdx: Option<&'static str>,
}

static LICENSE_MAPPING: LazyLock<HashMap<&'static str, LicenseId>> = LazyLock::new(|| {
    vec![
        (
            "Creative Commons - Attribution",
            (Some("cc"), Some("CC-BY-4.0")),
        ),
        (
            "Creative Commons - Attribution - Share Alike",
            (Some("cc-sa"), Some("CC-BY-SA-4.0")),
        ),
        (
            "Creative Commons - Attribution - No Derivatives",
            (Some("cc-nd"), None),
        ),
        (
            "Creative Commons - Attribution - Non-Commercial",
            (Some("cc-nc"), None),
        ),
        (
            "Creative Commons - Attribution - Non-Commercial - Share Alike",
            (Some("cc-nc-sa"), None),
        ),
        (
            "Creative Commons - Attribution - Non-Commercial - No Derivatives",
            (Some("cc-nc-nd"), None),
        ),
        (
            "Creative Commons - Share Alike",
            (Some("cc-sa"), Some("CC-BY-SA-4.0")),
        ),
        ("Creative Commons - No Derivatives", (Some("cc-nd"), None)),
        ("Creative Commons - Non-Commercial", (Some("cc-nc"), None)),
        (
            "Creative Commons - Non Commercial - Share alike",
            (Some("cc-nc-sa"), None),
        ),
        (
            "Creative Commons - Non Commercial - No Derivatives",
            (Some("cc-nc-nd"), None),
        ),
        (
            "Creative Commons - Public Domain Dedication",
            (Some("pd0"), Some("CC0-1.0")),
        ),
        ("Public Domain", (Some("public"), Some("CC0-1.0"))),
        ("GNU - GPL", (Some("gpl"), Some("GPL-3.0-or-later"))),
        ("GNU - LGPL", (Some("lgpl"), Some("LGPL-3.0-or-later"))),
        ("BSD", (Some("bsd"), Some("BSD-4-Clause"))),
        ("BSD License", (Some("bsd"), Some("BSD-4-Clause"))),
        ("Nokia", (Some("nokia"), None)),
        ("All Rights Reserved", (Some("none"), None)),
        ("Other", (None, None)),
        ("None", (None, None)),
    ]
    .into_iter()
    .map(|(tv_long, (thingiverse_short, spdx))| {
        (
            tv_long,
            LicenseId {
                thingiverse_short,
                spdx,
            },
        )
    })
    .collect()
});

type Int = isize;
type Url = String;
type DateTime = String;

/// Error message from the Thingiverse API,
/// coming to us as JSON.
#[derive(Deserialize, Debug)]
pub struct TvApiError {
    pub error: String,
}

pub static RE_ERR_MSG_DOES_NOT_EXIST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Thing (\d+) does not exist$").unwrap());

impl TvApiError {
    pub fn is_thing_does_not_exist(&self) -> bool {
        RE_ERR_MSG_DOES_NOT_EXIST.is_match(self.error.as_str())
        // self.error.starts_with("Thing ") && self.error.ends_with(" does not exist")
    }

    pub fn is_thing_has_not_been_published(&self) -> bool {
        self.error == "Thing has not been published"
    }

    pub fn is_thing_is_private(&self) -> bool {
        self.error == "Thing is private"
    }

    pub fn is_thing_is_under_moderation(&self) -> bool {
        self.error == "Thing is under moderation"
    }

    pub fn get_thing_id_if_not_exists(&self) -> Option<ThingId> {
        RE_ERR_MSG_DOES_NOT_EXIST
            .captures(&self.error)
            .and_then(|re_match| {
                let id_str = re_match.get(1).unwrap().as_str();
                id_str.parse().ok()
            })
    }
}

impl From<TvApiError> for Error {
    fn from(other: TvApiError) -> Self {
        if other.is_thing_has_not_been_published() {
            Self::ProjectDoesNotExist
        } else if other.is_thing_is_under_moderation() || other.is_thing_is_private() {
            Self::ProjectNotPublic
        } else if let Some(thing_id) = other.get_thing_id_if_not_exists() {
            Self::ProjectDoesNotExistId(HostingUnitId::WebById(HostingUnitIdWebById::from_parts(
                HostingProviderId::ThingiverseCom,
                thing_id.to_string(),
            )))
        } else {
            Self::HostingApiMsg(other.error)
        }
    }
}

impl Display for TvApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for TvApiError {}

#[derive(Deserialize, Debug)]
pub struct SearchSuccess {
    pub total: usize,
    pub hits: Vec<Thing>,
}

#[derive(Deserialize, Debug)]
pub struct Person {
    pub id: usize,
    pub name: String,
    pub first_name: String,
    pub last_name: String,
    pub url: Url,
    pub public_url: Url,
    pub thumbnail: Url,
    pub count_of_followers: Int,
    pub count_of_following: Int,
    pub count_of_designs: Int,
    pub make_count: Int,
    pub accepts_tips: bool,
    pub is_following: bool,
    pub location: String,
    pub cover: Url,
    pub is_admin: bool,
    pub is_moderator: bool,
    pub is_featured: bool,
    pub is_verified: bool,
}

#[derive(Deserialize, Debug)]
pub struct ImageSize {
    pub r#type: String,
    pub size: String,
    pub url: Url,
}

#[derive(Deserialize, Debug)]
pub struct Image {
    pub id: Int,
    pub url: Url,
    pub name: String,
    pub sizes: Vec<ImageSize>,
    pub added: DateTime,
}

#[derive(Deserialize, Debug)]
pub struct PartDatum {
    pub content: Option<String>,
    // NOTE The following are commented out because we don't need them right now; this saves us parsing time.
    // pub printer: Option<String>,
    // pub rafts: Option<String>,
    // pub supports: Option<String>,
    // pub resolution: Option<String>,
    // pub infill: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct PartDetails {
    pub r#type: String,
    pub name: String,
    // pub required: Option<String>, // TODO FIXME this can be either string or bool :/
    pub data: Option<Vec<PartDatum>>,
}

#[derive(Deserialize, Debug)]
pub struct Tag {
    pub name: String,
    pub url: Url,
    pub count: Int,
    pub things_url: Url,
    pub absolute_url: String,
}

#[derive(Deserialize, Debug)]
pub struct ZipFile {
    pub name: String,
    pub url: Url,
}

#[derive(Deserialize, Debug)]
pub struct ZipData {
    pub files: Option<Vec<ZipFile>>,
    pub images: Option<Vec<ZipFile>>,
}

#[derive(Deserialize, Debug)]
pub struct Thing {
    pub id: ThingId,
    pub name: String,
    pub thumbnail: Url,
    pub url: Url,
    pub public_url: Url,
    pub creator: Option<Person>,
    pub added: Option<DateTime>,
    pub modified: Option<DateTime>,
    pub is_published: Int,
    pub is_wip: Option<Int>,
    pub is_featured: bool,
    pub is_nsfw: Option<bool>,
    pub is_ai: bool,
    pub like_count: Int,
    pub is_liked: Option<bool>,
    pub collect_count: Int,
    pub is_collected: Option<bool>,
    pub comment_count: Int,
    pub is_watched: Option<bool>,
    pub default_image: Option<Image>,
    pub description: Option<String>,
    pub instructions: Option<String>,
    pub description_html: Option<String>,
    pub instructions_html: Option<String>,
    pub details: Option<String>,
    pub details_parts: Option<Vec<PartDetails>>,
    pub edu_details: Option<String>,
    pub edu_details_parts: Option<Vec<PartDetails>>,
    pub license: Option<String>,
    pub allows_derivatives: Option<bool>,
    pub files_url: Option<Url>,
    pub images_url: Option<Url>,
    pub likes_url: Option<Url>,
    pub ancestors_url: Option<Url>,
    pub derivatives_url: Option<Url>,
    pub tags_url: Option<String>,
    pub tags: Vec<Tag>,
    pub categories_url: Option<Url>,
    pub file_count: Option<Int>,
    pub is_purchased: Option<Int>,
    pub app_id: Option<Int>,
    pub download_count: Option<Int>,
    pub view_count: Option<Int>,
    pub education: Option<HashMap<String, Value>>,
    pub remix_count: Option<Int>,
    pub make_count: Option<Int>,
    pub app_count: Option<Int>,
    pub root_comment_count: Option<Int>,
    pub moderation: Option<String>,
    pub is_derivative: Option<bool>,
    pub ancestors: Option<Vec<Value>>,
    // pub can_comment: Option<bool>,
    pub type_name: Option<String>,
    pub is_banned: bool,
    pub is_comments_disabled: Option<bool>,
    pub needs_moderation: Option<Int>,
    pub is_decoy: Option<Int>,
    pub zip_data: Option<ZipData>,
}

// {
//     "total": 10000,
//     "hits": [
//       {
//         "id": 6975184,
//         "name": "Trump-Poetin",
//         "url": "https://api.thingiverse.com/things/6975184",
//         "public_url": "https://www.thingiverse.com/thing:6975184",
//         "created_at": "2025-03-10T20:12:44+00:00",
//         "thumbnail": "https://cdn.thingiverse.com/site/img/default/Gears_thumb_medium.jpg",
//         "preview_image": "https://cdn.thingiverse.com/site/img/default/Gears_preview_card.jpg",
//         "creator": {
//           "id": 644360,
//           "name": "adv51",
//           "first_name": "albert",
//           "last_name": "de vries",
//           "url": "https://api.thingiverse.com/users/adv51",
//           "public_url": "https://www.thingiverse.com/adv51",
//           "thumbnail": "https://cdn.thingiverse.com/renders/9e/97/79/26/c0/156005c5baf40ff51a327f1c34f2975b_thumb_medium.jpg",
//           "count_of_followers": 19,
//           "count_of_following": 16,
//           "count_of_designs": 56,
//           "collection_count": 4,
//           "make_count": 44,
//           "accepts_tips": true,
//           "is_following": false,
//           "location": "Netherlands",
//           "cover": "https://cdn.thingiverse.com/site/img/default/cover/cover-10_preview_large.jpg",
//           "is_admin": false,
//           "is_moderator": false,
//           "is_featured": false,
//           "is_verified": false
//         },
//         "is_private": 0,
//         "is_purchased": 0,
//         "is_published": 1,
//         "is_featured": false,
//         "is_edu_approved": null,
//         "is_printable": false,
//         "is_winner": false,
//         "allows_derivatives": true,
//         "comment_count": 0,
//         "make_count": 0,
//         "like_count": 0,
//         "tags": [
//           {
//             "name": "2D-art",
//             "tag": "2d-art",
//             "url": "https://api.thingiverse.com/tags/2d-art",
//             "count": 7,
//             "things_url": "https://api.thingiverse.com/tags/2d-art/things",
//             "absolute_url": "/tag:2D-art"
//           },
//           {
//             "name": "Poetin",
//             "tag": "poetin",
//             "url": "https://api.thingiverse.com/tags/poetin",
//             "count": 1,
//             "things_url": "https://api.thingiverse.com/tags/poetin/things",
//             "absolute_url": "/tag:Poetin"
//           },
//           {
//             "name": "TRUMP_",
//             "tag": "trump_",
//             "url": "https://api.thingiverse.com/tags/trump_",
//             "count": 9,
//             "things_url": "https://api.thingiverse.com/tags/trump_/things",
//             "absolute_url": "/tag:TRUMP_"
//           }
//         ],
//         "is_nsfw": null,
//         "is_ai": false,
//         "rank": null,
//         "collect_count": 0,
//         "moderation": "",
//         "is_banned": false,
//         "needs_moderation": 0,
//         "is_decoy": 0,
//         "is_comments_disabled": false
//       }
//     ]
//   }

impl Thing {
    #[must_use]
    pub fn is_open_source(&self) -> bool {
        self.spdx_license().is_some()
    }

    pub fn spdx_license(&self) -> Option<String> {
        self.license.as_ref()?;
        let license_raw = self.license.as_ref().unwrap();

        if ["None", "Other"].contains(&license_raw.as_str()) {
            return None;
        }

        let mapped_license_opt = LICENSE_MAPPING.get(license_raw.as_str());
        mapped_license_opt
            .and_then(|license_id| license_id.spdx)
            .map(std::borrow::ToOwned::to_owned)
    }
}

#[derive(Deserialize, Debug)]
struct File {
    pub id: Int,
    pub name: String,
    pub size: Int,
    pub url: Url,
    pub public_url: Url,
    pub download_url: Url,
    pub threejs_url: Url,
    pub thumbnail: Url,
    pub default_image: Image,
    pub date: DateTime,
    pub formatted_size: String,
    pub download_count: Int,
    pub direct_url: Url,
}
