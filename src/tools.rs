// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::borrow::Cow;

use reqwest::header::HeaderValue;

use urlencoding::encode;
// use url::form_urlencoded;

use once_cell::sync::Lazy;

pub static USER_AGENT_VALUE: Lazy<HeaderValue> = Lazy::new(|| {
    "okh-scraper github.com/iop-alliance/OpenKnowHow"
        .parse()
        .unwrap()
});

pub type SpdxLicenseExpression = Cow<'static, str>;

pub const LICENSE_UNKNOWN: SpdxLicenseExpression = Cow::Borrowed("LicenseRef-NOASSERTION");
pub const LICENSE_PROPRIETARY: SpdxLicenseExpression =
    Cow::Borrowed("LicenseRef-AllRightsReserved");
// __unknown_license__: License = License(
//     _id="LicenseRef-NOASSERTION",
//     name="No license statement is present; This equals legally to All Rights Reserved (== proprietary)",
//     reference_url="https://en.wikipedia.org/wiki/All_rights_reserved",
//     type_=LicenseType.UNKNOWN,
//     is_spdx=False,
//     is_osi_approved=False,
//     is_fsf_libre=False,
//     is_blocked=True,
// )
// __proprietary_license__: License = License(
//     _id="LicenseRef-AllRightsReserved",
//     name="All Rights Reserved (== proprietary)",
//     reference_url="https://en.wikipedia.org/wiki/All_rights_reserved",
//     type_=LicenseType.PROPRIETARY,
//     is_spdx=False,
//     is_osi_approved=False,
//     is_fsf_libre=False,
//     is_blocked=True,
// )

/// Returns the quoting character,
/// if the supplied string starts with one,
/// else returns `None`.
///
/// ```
/// # use okh_scraper::tools::url_encode;
/// assert_eq!(url_encode(r#"Hello World"#), "Hello%20World");
/// ```
#[must_use]
pub fn url_encode(input: &str) -> Cow<str> {
    encode(input)
    // form_urlencoded::byte_serialize(input.as_bytes()).collect()
}
