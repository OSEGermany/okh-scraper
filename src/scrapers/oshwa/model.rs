// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::tools::{SpdxLicenseExpression, LICENSE_UNKNOWN};
use serde::Deserialize;
use std::{borrow::Cow, fmt::Display};

#[derive(Deserialize, Debug)]
pub(super) struct OshwaProject {
    #[serde(rename = "oshwaUid")]
    pub(super) oshwa_uid: String,
}

#[derive(Deserialize, Debug)]
pub(super) struct Projects {
    pub(super) items: Vec<OshwaProject>,
    limit: usize,
    pub(super) total: usize,
}

#[derive(Deserialize, Debug)]
struct ErrorDetail {
    msg: String,
    param: String,
    location: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(super) struct ApiError {
    status_code: usize,
    error_code: String,
    message: String,
    details: Vec<ErrorDetail>,
}

impl Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:#?}")
    }
}

impl std::error::Error for ApiError {}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(super) struct ErrorCont {
    pub(super) error: ApiError,
}

#[derive(Deserialize, Debug)]
enum ResponsiblePartyType {
    Company,
    Organization,
    Individual,
}

#[derive(Deserialize, Debug, Clone, Copy)]
enum Category {
    Agriculture,
    Arts,
    Education,
    Electronics,
    Environmental,
    Iot,
    Manufacturing,
    Other,
    Science,
    Tool,
    Wearables,

    #[serde(rename = "3D Printing")]
    _3DPrinting,
    Enclosure,
    #[serde(rename = "Home Connection")]
    HomeConnection,
    Robotics,
    Sound,
    Space,
}

impl Category {
    pub(super) const fn to_cpc(self) -> Option<&'static str> {
        match self {
            Self::_3DPrinting => Some("B33Y"),
            Self::Enclosure => Some("F16M"),
            Self::HomeConnection => Some("H04W"),
            Self::Robotics => Some("B25J9/00"),
            Self::Sound => Some("H04R"),
            Self::Space => Some("B64G"),

            Self::Agriculture
            | Self::Arts
            | Self::Education
            | Self::Electronics
            | Self::Environmental
            | Self::Iot
            | Self::Manufacturing
            | Self::Other
            | Self::Science
            | Self::Tool
            | Self::Wearables => None,
        }
    }
}

/// License IDs as found on OSHWA.
///
/// NOTE: It is important to keep the serde names
/// consistent with what OSHWA uses,
/// _not_ with SPDX!
#[derive(Deserialize, Clone, Copy, Debug)]
enum OshwaLicense {
    #[serde(rename = "None")]
    None,
    #[serde(rename = "Other")]
    Other,
    #[serde(rename = "BSD-2-Clause")]
    Bsd2Clause,
    #[serde(rename = "CC0-1.0", alias = "CC 0")]
    Cc0_1_0,
    #[serde(rename = "CC-BY-4.0", alias = "CC BY")]
    CcBy4_0,
    #[serde(rename = "CC-BY-SA-4.0", alias = "CC BY-SA")]
    CcBySa4_0,
    #[serde(rename = "CERN OHL", alias = "CERN")]
    CernOhl,
    #[serde(rename = "GPL")]
    Gpl,
    #[serde(rename = "GPL-3.0")]
    Gpl3_0,
    Solderpad,
    #[serde(rename = "TAPR", alias = "OHL")]
    TaprOhl,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ApiProject {
    responsible_party_type: ResponsiblePartyType,
    responsible_party: String,
    public_contact: String,
    project_name: String,
    project_version: String,
    project_description: String,
    #[serde(rename = "oshwaUid")]
    oshwa_uid: String,
    hardware_license: Option<OshwaLicense>,
    documentation_license: Option<OshwaLicense>,
    software_license: Option<OshwaLicense>,
    primary_type: Category,
    additional_type: Category,
    country: String,
    certification_date: String, // TODO parse as "%Y-%m-%dT%H:%M%z"
}

impl ApiProject {
    pub(super) fn license(&self) -> Option<SpdxLicenseExpression> {
        let mut license = self.hardware_license;

        if license.is_none() {
            return Some(LICENSE_UNKNOWN);
        }

        if matches!(license, Some(OshwaLicense::Other)) {
            license = self.documentation_license;
        }

        if license.is_none()
            || matches!(license, Some(OshwaLicense::Other))
            || matches!(license, Some(OshwaLicense::None))
        {
            return Some(LICENSE_UNKNOWN);
        }

        license.and_then(OshwaLicense::to_spdx_expr).map(Cow::from)
    }
}

impl OshwaLicense {
    pub(super) const fn to_spdx_expr(self) -> Option<&'static str> {
        Some(match self {
            Self::Bsd2Clause => "BSD-2-Clause",
            // Self::CC0 => "CC0-1.0",
            Self::Cc0_1_0 => "CC0-1.0",
            // Self::CC_BY => "CC-BY-4.0",
            // Self::CC_BY_SA => "CC-BY-SA-4.0",
            Self::CcBy4_0 => "CC-BY-4.0",
            Self::CcBySa4_0 => "CC-BY-SA-4.0",
            // Self::CERN => "CERN-OHL-1.2",
            Self::CernOhl => "CERN-OHL-1.2",
            Self::Gpl => "GPL-3.0-or-later",
            Self::Gpl3_0 => "GPL-3.0-only",
            Self::Solderpad => "Apache-2.0 WITH SHL-2.1",
            Self::TaprOhl => "TAPR-OHL-1.0",
            Self::None | Self::Other => return None,
        })
    }
}

impl From<Projects> for Vec<String> {
    fn from(value: Projects) -> Self {
        value.items.iter().map(|p| p.oshwa_uid.clone()).collect()
    }
}
