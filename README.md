<!--
SPDX-FileCopyrightText: 2021-2025 Robin Vobruba <hoijui.quaero@gmail.com>

SPDX-License-Identifier: CC0-1.0
-->

# `okh-scraper`

[![License: AGPL-3.0-or-later](
    https://img.shields.io/badge/License-AGPL%203.0+-blue.svg)](
    LICENSE.txt)
[![REUSE status](
    https://api.reuse.software/badge/github.com/iop-alliance/okh-scraper)](
    https://api.reuse.software/info/github.com/iop-alliance/okh-scraper)
[![Repo](
    https://img.shields.io/badge/Repo-GitHub-555555&logo=github.svg)](
    https://github.com/iop-alliance/okh-scraper)
[![Package Releases](
    https://img.shields.io/crates/v/okh-scraper.svg)](
    https://crates.io/crates/okh-scraper)
[![Documentation Releases](
    https://docs.rs/okh-scraper/badge.svg)](
    https://docs.rs/okh-scraper)
[![Dependency Status](
    https://deps.rs/repo/github/iop-alliance/okh-scraper/status.svg)](
    https://deps.rs/repo/github/iop-alliance/okh-scraper)
[![Build Status](
    https://github.com/iop-alliance/okh-scraper/workflows/build/badge.svg)](
    https://github.com/iop-alliance/okh-scraper/actions)

[![In cooperation with Open Source Ecology Germany](
    https://raw.githubusercontent.com/osegermany/tiny-files/master/res/media/img/badge-oseg.svg)](
    https://opensourceecology.de)

A stand-alone service that scrapes(/crawls)
[Open Source Hardware (OSH)][OSH] projects
from different platforms
and other hosting technologies. \
The collected data conforms to the [Open Know-How (OKH)][OKH]
and the [Open Dataset (ODS)][ODS] standards.

## Usage

1. Fill out the config file (`config.yml`)
2. Start the scraper

It will continuously collect and update OSH project data,
found on the supported and configured platforms and other locations.
The fetched and the converted data,
plus the related scraping meta-data,
get stored in structured, text based file formats --
[JSON], [TOML], [YAML] and [Turtle] --
and committed and pushed to a git repo.
That repo is then synced with its forks,
which are directly pushed to by other instances of this scraper.
This basically constitutes a distributed scraping mechanism,
if configured correctly:
The different scraper instances should ideally al fetch all platforms,
but different "sections" of the total of the hosted projects.
For example in the case of Thingiverse,
which as of Early 2025 hosts about 3 million projects,
one crawler would grase the ID range of 0 to 499'999,
the second one from 500'000 to 999'999,
and so on.

## Building

```bash
# To get a binary for your system
cargo build --release

# To get a 64bit binary that is portable to all Linux systems
run/rp/build
```

## Testing

To run unit-, doc- and integration-tests:

```bash
run/rp/test
```

[ODS]: https://codeberg.org/elevont/open-dataset/
[OKH]: https://github.com/iop-alliance/OpenKnowHow/
[OSH]: https://www.opensourcehardware.org/
[JSON]: https://www.json.org/
[TOML]: https://toml.io/
[YAML]: https://yaml.org/
[Turtle]: https://www.w3.org/TR/turtle/
