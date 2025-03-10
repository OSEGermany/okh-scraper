// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

#![feature(async_closure)]

use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use async_walkdir::{DirEntry, Error as WDError, Filtering, WalkDir};
use regex::Regex;
use thiserror::Error;
// use futures_lite::future::block_on;
// use futures_lite::stream::StreamExt;
// use futures::{future::block_on, stream::BoxStream};
use futures::stream::StreamExt;
use futures::{stream::BoxStream, TryStreamExt};

#[derive(Error, Debug)]
pub enum FindError {
    #[error(transparent)]
    RegexError(#[from] regex::Error),
    #[error("File name has no extension")]
    NoFileExtension,
    #[error("Not a valid file name")]
    InvalidFileName,
    #[error("No valid base file")]
    InvalidBaseFile,
    #[error("An OS string is not valid utf-8")]
    OsStringNotUtf8,
    #[error(transparent)]
    IoError(#[from] std::io::Error),
}

// async fn is_manifest(entry: DirEntry) -> Filtering {
//     entry.file_name().to_str() {

//     }
// }

async fn finder(base: impl AsRef<Path>, file_name_pattern: Regex) -> BoxStream<'static, PathBuf> {
    let mut filter = move |entry: DirEntry| {
        let file_name_pattern = file_name_pattern.clone();
        async move {
            let path = entry.path();
            let file_name_opt = path.file_name().and_then(OsStr::to_str);
            if let Some(file_name) = file_name_opt {
                if file_name_pattern.is_match(file_name) {
                    return Filtering::Continue;
                }
            }
            Filtering::Ignore
        }
    };

    let mut entries = WalkDir::new(base).filter(filter);
    entries
        .into_stream()
        .filter_map(async move |entry: Result<DirEntry, WDError>| {
            entry
                .ok()
                .and_then(|arg0: async_walkdir::DirEntry| Some(DirEntry::path(&arg0)))
        })
        .boxed()
}

// fn finder(
//     base: impl AsRef<Path>,
//     filen_name_pattern: &Regex,
// ) -> Result<Vec<PathBuf>, Error> {
//     // let ignores = [".exe", ".psd", ".mp4"];

//     let matching_files: Vec<PathBuf> = WalkDir::new(path)
//         .into_iter()
//         .filter_map(|e| e.ok())
//         .filter(|e| e.file_name().to_str().map_or(
//             false, |s| ignores.into_iter().any(|i| s.ends_with(i))
//         ))
//         .map(|x| x.path().to_owned())
//         .collect();

//     Ok(matching_files)
// }

pub fn find_recursive(
    base: impl AsRef<Path>,
    filen_name_pattern: &Regex,
) -> Result<Vec<PathBuf>, FindError> {
    Ok(vec![])
    // // Ok(vec![]) // TODO FIXME
    // let base_path = base.as_ref().to_path_buf();
    // let mut dirs = vec![base_path];
    // while !dirs.is_empty() {
    // }
    // fs::read_dir(&base)?
    //     .filter_map(|entry| {
    //         match entry {
    //             Ok(entry) => {
    //             let entry_path = entry.path();
    //             if entry_path.is_dir() {
    //                 dirs.push(entry_path);
    //                 None
    //             } else {
    //                 Some(Ok((
    //                     entry_path
    //                         .file_name()
    //                         .ok_or(FindError::InvalidFileName)?
    //                         .to_str()
    //                         .ok_or(FindError::OsStringNotUtf8)?
    //                         .to_string(),
    //                     entry,
    //                 )))
    //             }
    //         },
    //         Err(err) => Some(Err(err)),
    //     }
    //     })
    //     Ok(
    //         .collect::<Result<Vec<_>, FindError>>()?
    //     .into_iter()
    //     .filter_map(|(file_name, entry)| {
    //         if let Ok(file_type) = entry.file_type() {
    //             if file_type.is_file() && filen_name_pattern.is_match(&file_name) {
    //                 return Some(entry.path());
    //             }
    //         }
    //         None
    //     })
    //     .collect())
}

pub fn find_recursive_simple(
    base_and_pattern: impl AsRef<Path>,
) -> Result<Vec<PathBuf>, FindError> {
    let path = base_and_pattern.as_ref();
    let base = path
        .parent()
        .ok_or(FindError::InvalidBaseFile)?
        .to_str()
        .ok_or(FindError::OsStringNotUtf8)?;
    let file_name = path
        .file_stem()
        .ok_or(FindError::InvalidFileName)?
        .to_str()
        .ok_or(FindError::OsStringNotUtf8)?;
    let file_extension = path
        .extension()
        .ok_or(FindError::NoFileExtension)?
        .to_str()
        .ok_or(FindError::OsStringNotUtf8)?;
    let file_name_pattern_str = format!(r"{file_name}\.\d{{3}}\.{file_extension}");
    let file_name_pattern = Regex::new(&file_name_pattern_str)?;
    find_recursive(base, &file_name_pattern)
}
