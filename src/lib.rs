// SPDX-FileCopyrightText: 2021-2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

// #![feature(specialization)]
// #![feature(min_specialization)]

pub mod files_finder;
pub mod model;
pub mod scrapers;
pub mod settings;
pub mod structured_content;
pub mod tools;

use git_version::git_version;

// This tests rust code in the README with doc-tests.
// Though, It will not appear in the generated documentation.
#[doc = include_str!("../README.md")]
#[cfg(doctest)]
pub struct ReadmeDoctests;

pub const VERSION: &str = git_version!(cargo_prefix = "", fallback = "unknown");
