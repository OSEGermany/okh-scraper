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
    tools::{SpdxLicenseExpression, LICENSE_UNKNOWN, USER_AGENT_VALUE},
};
use async_stream::stream;
use async_trait::async_trait;
use futures::{stream::BoxStream, stream::StreamExt};
use governor::{Quota, RateLimiter};
use regex::Regex;
use reqwest::{
    header::{HeaderMap, AUTHORIZATION, USER_AGENT},
    Client,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::LazyLock;
use std::{borrow::Cow, collections::HashMap, fmt::Display, rc::Rc, sync::Arc};
use tokio::time::Duration;
use tracing::instrument;

pub static LICENSE_MAPPING: LazyLock<
    HashMap<&'static str, (Option<&'static str>, Option<&'static str>)>,
> = LazyLock::new(|| {
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
    .collect()
});

// #[derive(Deserialize, Debug)]
// struct OshwaProject {
//     #[serde(rename = "oshwaUid")]
//    pub  oshwa_uid: String,
// }

// #[derive(Deserialize, Debug)]
// struct Projects {
//    pub  items: Vec<OshwaProject>,
//    pub  limit: usize,
//    pub  total: usize,
// }

// #[derive(Deserialize, Debug)]
// struct ErrorDetail {
//    pub  msg: String,
//    pub  param: String,
//    pub  location: String,
// }

// #[derive(Deserialize, Debug)]
// #[serde(rename_all = "camelCase")]
// pub struct Error {
//    pub  status_code: usize,
//    pub  error_code: String,
//    pub  message: String,
//    pub  details: Vec<ErrorDetail>,
// }

// impl Display for Error {
//    pub  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         write!(f, "{self:#?}")
//     }
// }

// impl std::error::Error for Error {}

// #[derive(Deserialize, Debug)]
// #[serde(rename_all = "camelCase")]
// struct ErrorCont {
//    pub  error: Error,
// }

// #[derive(Deserialize, Debug)]
// enum ResponsiblePartyType {
//     Company,
//     Organization,
//     Individual,
// }

// #[derive(Deserialize, Debug, Clone, Copy)]
// enum Category {
//     Agriculture,
//     Arts,
//     Education,
//     Electronics,
//     Environmental,
//     Iot,
//     Manufacturing,
//     Other,
//     Science,
//     Tool,
//     Wearables,

//     #[serde(rename = "3D Printing")]
//     _3DPrinting,
//     Enclosure,
//     #[serde(rename = "Home Connection")]
//     HomeConnection,
//     Robotics,
//     Sound,
//     Space,
// }

// impl Category {
//     pub const fn to_cpc(self) -> Option<&'static str> {
//         match self {
//             Self::_3DPrinting => Some("B33Y"),
//             Self::Enclosure => Some("F16M"),
//             Self::HomeConnection => Some("H04W"),
//             Self::Robotics => Some("B25J9/00"),
//             Self::Sound => Some("H04R"),
//             Self::Space => Some("B64G"),

//             Self::Agriculture
//             | Self::Arts
//             | Self::Education
//             | Self::Electronics
//             | Self::Environmental
//             | Self::Iot
//             | Self::Manufacturing
//             | Self::Other
//             | Self::Science
//             | Self::Tool
//             | Self::Wearables => None,
//         }
//     }
// }

// /// License IDs as found on OSHWA.
// ///
// /// NOTE: It is important to keep the serde names
// /// consistent with what OSHWA uses,
// /// _not_ with SPDX!
// #[derive(Deserialize, Clone, Copy, Debug)]
// enum OshwaLicense {
//     #[serde(rename = "None")]
//     None,
//     #[serde(rename = "Other")]
//     Other,
//     #[serde(rename = "BSD-2-Clause")]
//     Bsd2Clause,
//     #[serde(rename = "CC0-1.0", alias = "CC 0")]
//     Cc0_1_0,
//     #[serde(rename = "CC-BY-4.0", alias = "CC BY")]
//     CcBy4_0,
//     #[serde(rename = "CC-BY-SA-4.0", alias = "CC BY-SA")]
//     CcBySa4_0,
//     #[serde(rename = "CERN OHL", alias = "CERN")]
//     CernOhl,
//     #[serde(rename = "GPL")]
//     Gpl,
//     #[serde(rename = "GPL-3.0")]
//     Gpl3_0,
//     Solderpad,
//     #[serde(rename = "TAPR", alias = "OHL")]
//     TaprOhl,
// }

// #[derive(Deserialize, Debug)]
// #[serde(rename_all = "camelCase")]
// struct ApiProject {
//    pub  responsible_party_type: ResponsiblePartyType,
//    pub  responsible_party: String,
//    pub  public_contact: String,
//    pub  project_name: String,
//    pub  project_version: String,
//    pub  project_description: String,
//     #[serde(rename = "oshwaUid")]
//    pub  oshwa_uid: String,
//    pub  hardware_license: Option<OshwaLicense>,
//    pub  documentation_license: Option<OshwaLicense>,
//    pub  software_license: Option<OshwaLicense>,
//    pub  primary_type: Category,
//    pub  additional_type: Category,
//    pub  country: String,
//    pub  certification_date: String, // TODO parse as "%Y-%m-%dT%H:%M%z"
// }

// impl ApiProject {
//     pub fn license(&self) -> Option<SpdxLicenseExpression> {
//         let mut license = self.hardware_license;

//         if license.is_none() {
//             return Some(LICENSE_UNKNOWN);
//         }

//         if matches!(license, Some(OshwaLicense::Other)) {
//             license = self.documentation_license;
//         }

//         if license.is_none()
//             || matches!(license, Some(OshwaLicense::Other))
//             || matches!(license, Some(OshwaLicense::None))
//         {
//             return Some(LICENSE_UNKNOWN);
//         }

//         license.and_then(OshwaLicense::to_spdx_expr).map(Cow::from)
//     }
// }

// impl OshwaLicense {
//     pub const fn to_spdx_expr(self) -> Option<&'static str> {
//         Some(match self {
//             Self::Bsd2Clause => "BSD-2-Clause",
//             // Self::CC0 => "CC0-1.0",
//             Self::Cc0_1_0 => "CC0-1.0",
//             // Self::CC_BY => "CC-BY-4.0",
//             // Self::CC_BY_SA => "CC-BY-SA-4.0",
//             Self::CcBy4_0 => "CC-BY-4.0",
//             Self::CcBySa4_0 => "CC-BY-SA-4.0",
//             // Self::CERN => "CERN-OHL-1.2",
//             Self::CernOhl => "CERN-OHL-1.2",
//             Self::Gpl => "GPL-3.0-or-later",
//             Self::Gpl3_0 => "GPL-3.0-only",
//             Self::Solderpad => "Apache-2.0 WITH SHL-2.1",
//             Self::TaprOhl => "TAPR-OHL-1.0",
//             Self::None | Self::Other => return None,
//         })
//     }
// }

// // impl Projects {
// //     pub fn check_limit(&self) -> BoxResult<()> {
// //         if self.query.category_members.len() == self.limits.category_members {
// //             return Err(format!("Appropedia reached (and very likely surpassed) a total number of projects that is higher than the max fetch limit set in its API ({}); please inform the appropedia.org admins!", self.limits.category_members).into());
// //         }
// //         Ok(())
// //     }
// // }

// impl From<Projects> for Vec<String> {
//    pub  fn from(value: Projects) -> Self {
//         value.items.iter().map(|p| p.oshwa_uid.clone()).collect()
//     }
// }

type Int = isize;
type t_string = String;
type t_url = String;
type t_datetime = String;

#[derive(Deserialize, Debug)]
pub struct SearchError {
    pub error: t_string,
}

pub static RE_ERR_MSG_DOES_NOT_EXIST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Thing (\d+) does not exist$").unwrap());

impl SearchError {
    pub fn is_thing_does_not_exist(&self) -> bool {
        RE_ERR_MSG_DOES_NOT_EXIST.is_match(self.error.as_str())
        // self.error.starts_with("Thing ") && self.error.ends_with(" does not exist")
    }

    pub fn get_thing_id_if_not_exists(&self) -> Option<ThingId> {
        if let Some(re_match) = RE_ERR_MSG_DOES_NOT_EXIST.captures(&self.error) {
            // let id_str = self.error.remove_matches("Thing ").remove_matches(" does not exist");
            let id_str = re_match.get(1).unwrap().as_str();
            id_str.parse().ok()
        } else {
            None
        }
    }
}

impl From<SearchError> for Error {
    fn from(other: SearchError) -> Self {
        if let Some(thing_id) = other.get_thing_id_if_not_exists() {
            Self::ProjectDoesNotExist(HostingUnitId::WebById(HostingUnitIdWebById::from_parts(
                HostingProviderId::ThingiverseCom,
                thing_id.to_string(),
            )))
        } else {
            Self::HostingApiMsg(other.error)
        }
    }
}

impl Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for SearchError {}

#[derive(Deserialize, Debug)]
pub struct SearchSuccess {
    pub total: usize,
    pub hits: Vec<Thing>,
}

#[derive(Deserialize, Debug)]
pub struct Person {
    pub id: usize,
    pub name: t_string,
    pub first_name: t_string,
    pub last_name: t_string,
    pub url: t_url,
    pub public_url: t_url,
    pub thumbnail: t_url,
    pub count_of_followers: Int,
    pub count_of_following: Int,
    pub count_of_designs: Int,
    pub make_count: Int,
    pub accepts_tips: bool,
    pub is_following: bool,
    pub location: t_string,
    pub cover: t_url,
    pub is_admin: bool,
    pub is_moderator: bool,
    pub is_featured: bool,
    pub is_verified: bool,
}

#[derive(Deserialize, Debug)]
pub struct ImageSize {
    pub r#type: t_string,
    pub size: t_string,
    pub url: t_url,
}

#[derive(Deserialize, Debug)]
pub struct Image {
    pub id: Int,
    pub url: t_url,
    pub name: t_string,
    pub sizes: Vec<ImageSize>,
    pub added: t_datetime,
}

#[derive(Deserialize, Debug)]
pub struct PartDatum {
    pub content: t_string,
}

#[derive(Deserialize, Debug)]
pub struct PartDetails {
    pub r#type: t_string,
    pub name: t_string,
    // pub required: Option<t_string>, // TODO FIXME this can be either string or bool :/
    pub data: Option<Vec<PartDatum>>,
}

#[derive(Deserialize, Debug)]
pub struct Tag {
    pub name: t_string,
    pub url: t_url,
    pub count: Int,
    pub things_url: t_url,
    pub absolute_url: t_string,
}

#[derive(Deserialize, Debug)]
pub struct ZipFile {
    pub name: t_string,
    pub url: t_url,
}

#[derive(Deserialize, Debug)]
pub struct ZipData {
    pub files: Option<Vec<ZipFile>>,
    pub images: Option<Vec<ZipFile>>,
}

#[derive(Deserialize, Debug)]
pub struct Thing {
    pub id: ThingId,
    pub name: t_string,
    pub thumbnail: t_url,
    pub url: t_url,
    pub public_url: t_url,
    pub creator: Person,
    pub added: Option<t_datetime>,
    pub modified: Option<t_datetime>,
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
    pub description: Option<t_string>,
    pub instructions: Option<t_string>,
    pub description_html: Option<t_string>,
    pub instructions_html: Option<t_string>,
    pub details: Option<t_string>,
    pub details_parts: Option<Vec<PartDetails>>,
    pub edu_details: Option<t_string>,
    pub edu_details_parts: Option<Vec<PartDetails>>,
    pub license: Option<t_string>,
    pub allows_derivatives: Option<bool>,
    pub files_url: Option<t_url>,
    pub images_url: Option<t_url>,
    pub likes_url: Option<t_url>,
    pub ancestors_url: Option<t_url>,
    pub derivatives_url: Option<t_url>,
    pub tags_url: Option<t_string>,
    pub tags: Vec<Tag>,
    pub categories_url: Option<t_url>,
    pub file_count: Option<Int>,
    pub is_purchased: Option<Int>,
    pub app_id: Option<t_string>,
    pub download_count: Option<Int>,
    pub view_count: Option<Int>,
    pub education: Option<HashMap<t_string, Value>>,
    pub remix_count: Option<Int>,
    pub make_count: Option<Int>,
    pub app_count: Option<Int>,
    pub root_comment_count: Option<Int>,
    pub moderation: Option<t_string>,
    pub is_derivative: Option<bool>,
    pub ancestors: Option<Vec<Value>>,
    // pub can_comment: Option<bool>,
    pub type_name: Option<t_string>,
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
            .and_then(|(_short_license, spdx_license)| spdx_license.as_deref())
            .map(std::borrow::ToOwned::to_owned)
    }
}

#[derive(Deserialize, Debug)]
struct File {
    pub id: Int,
    pub name: t_string,
    pub size: Int,
    pub url: t_url,
    pub public_url: t_url,
    pub download_url: t_url,
    pub threejs_url: t_url,
    pub thumbnail: t_url,
    pub default_image: Image,
    pub date: t_datetime,
    pub formatted_size: t_string,
    pub download_count: Int,
    pub direct_url: t_url,
}
