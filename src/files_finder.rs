// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

#![feature(async_closure)]

use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use async_walkdir::{DirEntry, Error as WDError, Filtering, WalkDir};
use futures::stream::StreamExt;
use futures::{stream::BoxStream, TryStreamExt};
use regex::Regex;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FindError {
    #[error(transparent)]
    RegexError(#[from] regex::Error),
    #[error("Search-pattern file-name has no extension: '{0:?}'")]
    NoFileExtension(PathBuf),
    #[error("Not a valid file name (ends with '..'?): '{0:?}'")]
    InvalidFileName(PathBuf),
    #[error("The search-pattern misses a parent directory: '{0:?}'")]
    PatternMissingParent(PathBuf),
    #[error("The search-pattern misses a file-name template to search for: '{0:?}'")]
    PathMissingFileName(PathBuf),
    #[error("The search-pattern misses a file-stem template to search for: '{0:?}'")]
    PathMissingFileStem(PathBuf),
    #[error("Could not get file type for '{0:?}'")]
    FileType(PathBuf),
    #[error("A path is not valid UTF-8: '{0:?}'")]
    PathNotUtf8(PathBuf),
    #[error("Path part {0} is not valid UTF-8: '{1:?}'")]
    PathPartNotUtf8(&'static str, OsString),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
}

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

fn path_part_to_string(
    path_part_name: &'static str,
    path_part: &OsStr,
) -> Result<String, FindError> {
    path_part
        .to_str()
        .ok_or_else(|| FindError::PathPartNotUtf8(path_part_name, path_part.to_os_string()))
        .map(ToOwned::to_owned)
}

fn extract_file_name(path: impl AsRef<Path>) -> Result<String, FindError> {
    let path = path.as_ref();
    let raw = path
        .file_name()
        .ok_or_else(|| FindError::PathMissingFileName(path.to_path_buf()))?;
    path_part_to_string("file-name", raw)
}

fn extract_file_stem(path: impl AsRef<Path>) -> Result<String, FindError> {
    let path = path.as_ref();
    let raw = path
        .file_stem()
        .ok_or_else(|| FindError::PathMissingFileStem(path.to_path_buf()))?;
    path_part_to_string("file-stem", raw)
}

fn extract_file_extension(path: impl AsRef<Path>) -> Result<String, FindError> {
    let path = path.as_ref();
    let raw = path
        .extension()
        .ok_or_else(|| FindError::NoFileExtension(path.to_path_buf()))?;
    path_part_to_string("file-extension", raw)
}

pub fn find_recursive(
    base: impl AsRef<Path>,
    file_name_pattern: &Regex,
) -> Result<Vec<PathBuf>, FindError> {
    let base_path = base.as_ref().to_path_buf();
    let mut dirs = vec![base_path];
    let mut files = vec![];
    tracing::trace!("find_recursive ...");
    while let Some(dir) = dirs.pop() {
        tracing::trace!("find_recursive - dir: '{dir:?}' ...");
        files.append(
            &mut fs::read_dir(dir)?
                .filter_map(|entry| match entry {
                    Ok(entry) => {
                        let entry_path = entry.path();
                        let file_type_res = entry
                            .file_type()
                            .map_err(|_err| FindError::FileType(entry_path.clone()));
                        match file_type_res {
                            Ok(file_type) => {
                                if file_type.is_dir() {
                                    dirs.push(entry_path);
                                } else if file_type.is_file() {
                                    let file_name_res = extract_file_name(&entry_path);
                                    if let Ok(file_name) = file_name_res {
                                        tracing::trace!(
                                            "find_recursive - matching file: '{file_name:?}' ..."
                                        );
                                        if file_name_pattern.is_match(&file_name) {
                                            return Some(Ok(entry_path));
                                        }
                                    }
                                }
                                None
                            }
                            Err(_err) => None,
                        }
                    }
                    Err(err) => Some(Err(FindError::from(err))),
                })
                .collect::<Result<Vec<_>, FindError>>()?,
        );
    }
    tracing::trace!("find_recursive done.");
    Ok(files)
}

pub fn find_recursive_simple(
    base_and_pattern: impl AsRef<Path>,
) -> Result<Vec<PathBuf>, FindError> {
    let path = base_and_pattern.as_ref();
    let base = path
        .parent()
        .ok_or_else(|| FindError::PatternMissingParent(path.to_path_buf()))?;
    let base = base
        .to_str()
        .ok_or_else(|| FindError::PathNotUtf8(base.to_path_buf()))?;
    let file_stem = extract_file_stem(path)?;
    let file_extension = extract_file_extension(path)?;
    let file_name_pattern_str = format!(r"{file_stem}\.\d{{3}}\.{file_extension}");
    let file_name_pattern = Regex::new(&file_name_pattern_str)?;
    find_recursive(base, &file_name_pattern)
}
