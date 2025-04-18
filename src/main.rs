// SPDX-FileCopyrightText: 2021-2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

mod cli;
mod stream_test;

use async_std::{fs, io, path};
use clap::crate_name;
use cli_utils::BoxError;
use fs4::async_std::AsyncFileExt;
use futures::{pin_mut, Stream, StreamExt};
use std::pin::pin;
use thiserror::Error;
// use futures::future::select_all;
use futures::stream::select_all;
use okh_scraper::settings::{self, SettingsError};

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

#[derive(Debug, Clone, Copy)]
pub enum LockingErrorKind {
    CreateFile,
    OpenFile,
    Locking,
    Unlocking,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to initialize the logging/tracing system (1/2): {0}")]
    LoggingInit(#[from] tracing_subscriber::util::TryInitError),
    #[error("Failed to initialize the logging/tracing system (2/2): {0}")]
    LoggingInit2(#[from] tracing_subscriber::reload::Error),
    #[error("Failed to ensure exclusivity (locking): Is there already a scraper running (on the same machine or in an (other) container/VM?)? - error-kind: {0:#?}, lock-file: '{1:#?}'")]
    Locking(LockingErrorKind, path::PathBuf, Option<BoxError>),
    #[error("Failed to initialize/construct the configuration: {0}")]
    Settings(#[from] SettingsError),
    #[error("Database/Workdir '{0:#?}' does not exist")]
    MissingDatabaseDir(path::PathBuf),
}

macro_rules! lock_file {
    ($path_var:ident, $file_var:ident) => {
        if !$path_var.exists().await {
            fs::File::create(&$path_var).await.map_err(|err| {
                Error::Locking(
                    LockingErrorKind::CreateFile,
                    $path_var.clone(),
                    Some(err.into()),
                )
            })?;
        }
        tracing::debug!("Preparing to lock file '{}' ...", $path_var.display());
        let $file_var = fs::File::open(&$path_var).await.map_err(|err| {
            Error::Locking(
                LockingErrorKind::OpenFile,
                $path_var.clone(),
                Some(err.into()),
            )
        })?;
        if !$file_var.try_lock_exclusive().map_err(|err| {
            Error::Locking(
                LockingErrorKind::Locking,
                $path_var.clone(),
                Some(err.into()),
            )
        })? {
            return Err(Error::Locking(
                LockingErrorKind::Locking,
                $path_var.clone(),
                None,
            ));
        }
        tracing::debug!("Obtained lock on file '{}'.", $path_var.display());
    };
}

macro_rules! unlock_file {
    ($path_var:ident, $file_var:ident) => {
        tracing::trace!("Releasing lock on file '{}' ...", $path_var.display());
        $file_var.unlock().map_err(|err| {
            Error::Locking(
                LockingErrorKind::Unlocking,
                $path_var.clone(),
                Some(err.into()),
            )
        })?;
        tracing::info!("Released lock on file '{}'.", $path_var.display());
    };
}

#[tokio::main]
#[instrument]
async fn main() -> Result<(), Error> {
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

    // With this we try to enforce
    // that at most one scraper instance is running at a time,
    // on this system.
    let system_lock_file_path = path::PathBuf::from("/tmp/okh-scraper.lock");
    lock_file!(system_lock_file_path, system_lock_file);

    let run_settings = settings::load()?;
    // create the async version of the workdir path
    let workdir = path::PathBuf::from(&run_settings.database.path);
    // ensure the workdir exists
    if !workdir.exists().await {
        return Err(Error::MissingDatabaseDir(workdir));
    }

    // With this we try to enforce
    // that at most one scraper instance is running at a time,
    // that uses this workdir.
    //
    // We need both these lock files,
    // because otherwise one might run the scraper once (or more times) in a container,
    // and at the same time on the host machine, using the same workdir,
    // which would mess up the storage completely.
    //
    // If we had only this one nad not the former,
    // One might run the scraper multiple times on the same machine,
    // using different workdirs,
    // which would under-cut the API requests per minute quotas.
    //
    // NOTE This way, one could still run one scraper in a container
    //      and one on the host, using different workdirs.
    //      -> Don't do that!
    let workdir_lock_file_path = workdir.join("okh-scraper-workdir.lock");
    lock_file!(workdir_lock_file_path, workdir_lock_file);

    let mut scrape_streams = Vec::new();
    // TODO Parallelize this loop
    tracing::info!("Setting up scrapers ...");
    for (scraper_id, scraper) in run_settings.scrapers {
        tracing::info!("- Setting up scraper {scraper_id} ...");
        scrape_streams.push(scraper.scrape().await);
    }

    // stream_test::test().await;

    // let scrapers = run_settings.scrapers.into_iter().map(|(scraper_id, scraper)| async move {scraper.scrape_all().await});
    let mut projects = select_all(scrape_streams);

    // pin_mut!(projects); // needed for iteration
    while let Some(project) = projects.next().await {
        match project {
            Ok(proj) => println!("Scraped project: {}", proj.id),
            Err(err) => {
                if err.aborts() {
                    println!("Scraping error:\n{err}");
                } else {
                    println!("Scraping failed:\n{err}");
                }
            }
        }
    }

    unlock_file!(system_lock_file_path, system_lock_file);
    unlock_file!(workdir_lock_file_path, workdir_lock_file);

    // scrapers::appropedia::scrape_all().await?;
    // scrapers::oshwa::scrape_all().await?;

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
