// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostingCategory {
    Forge,
    Other,
}

impl AsRef<str> for HostingCategory {
    fn as_ref(&self) -> &str {
        match self {
            Self::Forge => "cat|Forge",
            Self::Other => "cat|Other",
        }
    }
}

impl fmt::Display for HostingCategory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}
