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

## Using

### How to get a Thingiverse REST API access token

To be able to scrape thingiverse
(similar to many other platforms),
you first need to generate an API access token,
which you then insert into this scrapers configuration,
so it can authorize itsself with the API.

**NOTE**
Every few months, you might need to generate a new such token!

How to generate one:

1. create a thingiverse.com account and log in with it
2. go to <https://www.thingiverse.com/developers/my-apps>
3. click on the "Create an App" button
4. choose "Web App"
5. Fill out the form:
    choose random values for name and description,
    click the "I agree to the MakerBot API terms and Privacy Policy" box,
    and click on the button "CREATE & GET APP KEY"
    on the top of the page, next to "CANCEL".
6. If all went well, your "APP" will have been created,
    and you will be presented with its details.
    We are interested in the **App Token** value.
    It is a 32 characters long string like this: \
    `1234567890abcdef1234567890abcdef` \
    copy it!

In the next sections, we will see how to use it.
Enter your access token into the configuration,
replacing the string `XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX`.

We will now learn how to run the scraper:

### Setup

#### Using podman/docker

```bash
if which podman > /dev/null
then
  cont_app=podman
elif which docker > /dev/null
then
  cont_app=docker
else
  >&2 echo "ERROR: Neither podman nor docker are in PATH"
fi
if ! which okh-scraper > /dev/null
then
  alias okh-scraper="$cont_app"' run --interactive --volume "$PWD:/home/okh-scraper" oseg/okh-scraper:master'
fi
```

#### Compiling from source

You need to [install Rust and Cargo],
if you don't already have it.

Then you have to get the sources, either through [git]:

```shell
git clone https://github.com/OSEGermany/okh-scraper.git
https://github.com/OSEGermany/okh-scraper/archive/refs/heads/master.zip
```

or by downloading [the sources as a ZIP file](
  https://github.com/OSEGermany/okh-scraper/archive/refs/heads/master.zip)
and extracting them.

After that, you change to the sources dir and compile and install:

```shell
cd okh-scraper
cargo install --path .
```

NOTE
To get a 64bit binary that is portable to all Linux systems,
use

```bash
# setup (only execute this once)
mkdir -p run
git clone https://github.com/hoijui/rust-project-scripts.git run/rp
run/rp/setup

# building
run/rp/build
```

### Running

```bash
mkdir okh-scraper-root
cd okh-scraper-root

cat > config.yml << EOF
user_agent: "new-projects-fetcherfetcher"

database:
  type: file       # (opt) nothing else implemented
  path: ./workdir  # (opt)

scrapers:
  thingiverse.com:
    scraper_type: thingiverse
    config:
      retries: 3   # (opt) scraper specific number of retries
      timeout: 15000  # (opt) scraper specific request timeout in milliseconds [ms]
      access_token: XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX  # (req) app access token to use the Thingiverse API
      things_store_root: ./workdir/thingiverse_store/  # (req) Where to store the raw thingiverse API scraping results to
      things_range_min: 2000000  # (opt)
      things_range_max: 2999999  # (opt)
EOF
```

1. Fill out the config file (`config.yml`)
2. Make sure that the dir specified in the configuration under `database:path` exists,
    in our case:
    `mkdir ./workdir`
3. Start the scraper: `okh-scraper`
4. Results will (hopefully) start trickling in under `./workdir`.

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

```bash
okh-scraper --verbose
```

## Testing

To run unit-, doc- and integration-tests:

```bash
run/rp/test
```

[install Rust and Cargo]: https://cargo-book.irust.net/en-us/getting-started/installation.html
[git]: https://git-scm.com/
[ODS]: https://codeberg.org/elevont/open-dataset/
[OKH]: https://github.com/iop-alliance/OpenKnowHow/
[OSH]: https://www.opensourcehardware.org/
[JSON]: https://www.json.org/
[TOML]: https://toml.io/
[YAML]: https://yaml.org/
[Turtle]: https://www.w3.org/TR/turtle/
