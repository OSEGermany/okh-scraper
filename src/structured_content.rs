// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use cli_utils::{BoxError, BoxResult};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Type of structure of data.
#[derive(Debug, Clone, Copy)]
pub enum Type {
    Rdf,
    /// Hierarchical data formats/Tree-structured data formats/Nested data formats.
    /// This includes primarily JSON, YAML and TOML.
    Tree,
}

#[derive(Debug, Clone, Copy)]
pub enum SerializationFormat {
    Turtle,
    Yaml,
    Json,
    Toml,
}

impl From<SerializationFormat> for Type {
    fn from(value: SerializationFormat) -> Self {
        match value {
            SerializationFormat::Turtle => Self::Rdf,
            SerializationFormat::Yaml | SerializationFormat::Json | SerializationFormat::Toml => {
                Self::Tree
            }
        }
    }
}

impl TryFrom<&Path> for SerializationFormat {
    type Error = BoxError;

    fn try_from(value: &Path) -> Result<Self, Self::Error> {
        let file_ext = value
            .extension()
            .ok_or("Path is not a file-path or the file has no extension")?
            .to_str()
            .ok_or("File-extension is not valid UTF-8")?
            .to_lowercase();
        Ok(match file_ext.as_str() {
            "yml" | "yaml" => Self::Yaml,
            "json" => Self::Json,
            "toml" => Self::Toml,
            "ttl" => Self::Turtle,
            _ => return Err(format!("Unsupported file extension '{file_ext}'").into()),
        })
    }
}

#[derive(Debug)]
pub enum RawContent {
    String(String),
    Bytes(Vec<u8>),
}

impl RawContent {
    #[must_use]
    pub fn as_string(&self) -> &str {
        match self {
            Self::String(s) => s,
            Self::Bytes(b) => std::str::from_utf8(b).unwrap(),
        }
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::String(s) => s.as_bytes(),
            Self::Bytes(b) => b,
        }
    }
}

impl From<String> for RawContent {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<Vec<u8>> for RawContent {
    fn from(value: Vec<u8>) -> Self {
        Self::Bytes(value)
    }
}

#[derive(Debug)]
pub struct Chunk<P: Serialize + DeserializeOwned> {
    format: SerializationFormat,
    content: Option<RawContent>,
    file: Option<PathBuf>,
    pub parsed: Option<P>,
}

impl<P: Serialize + DeserializeOwned> Chunk<P> {
    pub fn from_file(file: PathBuf) -> BoxResult<Self> {
        Ok(Self {
            format: SerializationFormat::try_from(file.as_path())?,
            content: None,
            file: Some(file),
            parsed: None,
        })
    }

    #[must_use]
    pub const fn from_content(format: SerializationFormat, content: RawContent) -> Self {
        Self {
            format,
            content: Some(content),
            file: None,
            parsed: None,
        }
    }

    pub const fn format(&self) -> SerializationFormat {
        self.format
    }

    pub const fn file(&self) -> Option<&PathBuf> {
        self.file.as_ref()
    }

    pub fn content(&mut self) -> &RawContent {
        if self.content.is_none() {
            match self.file.as_ref() {
                Some(file) => {
                    let file_contents_str = fs::read_to_string(file).unwrap();
                    let content = RawContent::String(file_contents_str);
                    self.content = Some(content);
                }
                None => panic!("Neither content nor file are given; this should not be possible"),
            }
        }
        self.content.as_ref().unwrap()
    }
}
