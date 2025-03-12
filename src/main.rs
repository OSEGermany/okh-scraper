// SPDX-FileCopyrightText: 2021-2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

mod cli;
mod stream_test;

use async_std::{fs, path};
use clap::crate_name;
use cli_utils::BoxResult;
use fs4::async_std::AsyncFileExt;
use futures::{pin_mut, Stream, StreamExt};
use std::pin::pin;
// use futures::future::select_all;
use futures::stream::select_all;
use okh_scraper::settings;

use cli_utils::logging;
use tracing::instrument;
// use okh_scraper::Settings;
use tracing_subscriber::filter::LevelFilter;
// use std::collections::HashMap;

#[allow(clippy::print_stdout)]
fn print_version_and_exit(quiet: bool) {
    if !quiet {
        print!("{} ", clap::crate_name!());
    }
    println!("{}", okh_scraper::VERSION);
    std::process::exit(0);
}

#[tokio::main]
#[instrument]
async fn main() -> BoxResult<()> {
    let log_reload_handle = logging::setup(crate_name!())?;
    let args = cli::args_matcher().get_matches();

    let quiet = args.get_flag(cli::A_L_QUIET);
    let version = args.get_flag(cli::A_L_VERSION);
    if version {
        print_version_and_exit(quiet);
    }

    let verbose = args.get_flag(cli::A_L_VERBOSE);

    let log_level = if verbose {
        // LevelFilter::DEBUG
        LevelFilter::TRACE
    } else if quiet {
        LevelFilter::WARN
    } else {
        LevelFilter::INFO
    };
    logging::set_log_level_tracing(&log_reload_handle, log_level)?;
    let list = args.get_flag(cli::A_L_LIST);
    let src = args.get_one::<String>(cli::A_L_INPUT).cloned();
    let dst = args.get_one::<String>(cli::A_L_OUTPUT).cloned();

    // TODO choose a good file name (e.g. /tmp/okh-scraper_thingiverse.lock)
    let lock_file_path = path::PathBuf::from("/tmp/okh-scraper.lock");
    if !lock_file_path.exists().await {
        fs::File::create(&lock_file_path).await?;
    }

    tracing::debug!("Preparing to lock file '{}' ...", lock_file_path.display());
    // let lock_file = fs::OpenOptions::new()
    //     .create(true)
    //     .open(&lock_file_path).await?;
    let lock_file = fs::File::open(&lock_file_path).await?;
    if !lock_file.try_lock_exclusive()? {
        return Err(format!("Failed to lock file '{}'", lock_file_path.display()).into());
    }
    tracing::debug!("Obtained lock on file '{}'.", lock_file_path.display());

    let run_settings = settings::load()?;
    let mut fetch_streams = Vec::new();
    // TODO Parallelize this loop
    tracing::info!("Setting up fetchers ...");
    for (fetcher_id, fetcher) in run_settings.fetchers {
        tracing::info!("- Setting up fetcher {fetcher_id} ...");
        fetch_streams.push(fetcher.fetch_all().await);
    }

    // stream_test::test().await;

    // let fetchers = run_settings.fetchers.into_iter().map(|(fetcher_id, fetcher)| async move {fetcher.fetch_all().await});
    let mut projects = select_all(fetch_streams);

    // pin_mut!(projects); // needed for iteration
    while let Some(project) = projects.next().await {
        match project {
            Ok(proj) => println!("Scraped project:\n{proj:#?}"),
            Err(err) => println!("Scraping error:\n{err:#?}"),
        }
    }

    tracing::trace!("Releasing lock on file '{}' ...", lock_file_path.display());
    lock_file.unlock()?;
    tracing::info!("Released lock on file '{}'.", lock_file_path.display());

    // fetchers::appropedia::fetch_all().await?;
    // fetchers::oshwa::fetch_all().await?;

    // if list {
    //     let detected_vars = replacer::extract_from_file(src.as_deref())?;
    //     tools::write_to_file(detected_vars, dst.as_deref())?;
    // } else {
    //     let mut vars = HashMap::new();

    //     // enlist environment variables
    //     if args.get_flag(cli::A_L_ENVIRONMENT) {
    //         tools::append_env(&mut vars);
    //     }
    //     // enlist variables from files
    //     if let Some(var_files) = args.get_many::<String>(cli::A_L_VARIABLES_FILE) {
    //         for var_file in var_files {
    //             let mut reader = cli_utils::create_input_reader(Some(var_file))?;
    //             vars.extend(key_value::parse_vars_file_reader(&mut reader)?);
    //         }
    //     }
    //     // enlist variables provided on the CLI
    //     if let Some(variables) = args.get_many::<String>(cli::A_L_VARIABLE) {
    //         for key_value in variables {
    //             let pair = key_value::Pair::parse(key_value)?;
    //             vars.insert(pair.key.to_owned(), pair.value.to_owned());
    //         }
    //     }

    //     let fail_on_missing = args.get_flag(cli::A_L_FAIL_ON_MISSING_VALUES);

    //     let settings = settings! {
    //         vars: vars,
    //         fail_on_missing: fail_on_missing
    //     };

    //     replacer::replace_in_file(src.as_deref(), dst.as_deref(), &settings)?;
    // }

    Ok(())
}
