// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

#![allow(clippy::shadow_reuse)]

use crate::scrapers::{self, Scraper};
use config::{Config, ConfigError};
use reqwest_retry::RetryTransientMiddleware;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::io::{self, BufRead, Write};
use std::{collections::HashMap, path::PathBuf, rc::Rc, sync::Arc};
use thiserror::Error;
use typed_builder::TypedBuilder;

#[derive(Error, Debug)]
pub enum SettingsError {
    #[error("Failed to load the basic/low-level configuration data: {0}")]
    Config(#[from] ConfigError),
    #[error("Failed to create a scraper from the basic/low-level configuration data: {0}")]
    ScraperCreation(#[from] scrapers::CreationError),
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseType {
    File,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Database {
    pub r#type: DatabaseType,
    pub path: PathBuf,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct IntermediateSettings {
    pub user_agent: String,
    pub database: Database,
    pub fetchers: HashMap<String, HashMap<String, Value>>,
}

#[derive(Debug)]
pub struct PartialSettings {
    pub user_agent: String,
    pub database: Database,
}

#[derive(TypedBuilder)]
pub struct Settings {
    pub user_agent: String,
    pub database: Database,
    pub fetchers: HashMap<String, Box<dyn Scraper>>,
}

impl IntermediateSettings {
    #[must_use]
    pub fn partial(&self) -> PartialSettings {
        PartialSettings {
            user_agent: self.user_agent.clone(),
            database: self.database.clone(),
        }
    }

    pub fn finalize(self) -> Result<Settings, SettingsError> {
        let fetcher_factories = scrapers::assemble_factories();
        let mut fetchers = HashMap::new();
        let config_partial = Arc::new(self.partial());
        for (fetcher_id, properties) in self.fetchers {
            let fetcher_type = properties
                .get("fetcher_type")
                .expect("fetcher section requires property 'fetcher_type'")
                .as_str()
                .expect("property 'fetcher_type' needs to be a string");
            tracing::debug!("Fetcher '{fetcher_id}' has type: '{fetcher_type}' - parsing ...");
            if [
                "none",
                "default",
                "forgejo",
                "github",
                "gitlab",
                // "thingiverse",
                "oshwa",
                "appropedia",
            ]
            .contains(&fetcher_type)
            {
                // TODO HACK
                tracing::debug!(
                    "ignoring '{fetcher_id}' because type not yet implemented: '{fetcher_type}'"
                );
                continue;
            }
            let factory = fetcher_factories
                .get(fetcher_type)
                .unwrap_or_else(|| panic!("No fetcher found for type '{fetcher_type}'"));
            let fetcher = factory.create(
                Arc::<PartialSettings>::clone(&config_partial),
                properties
                    .get("config")
                    .unwrap_or_else(|| panic!("No config found for fetcher '{fetcher_id}'"))
                    .clone(),
            )?;
            fetchers.insert(fetcher_id, fetcher);
        }

        Ok(Settings {
            user_agent: self.user_agent,
            database: self.database,
            fetchers,
        })
    }
}

/// # Errors
///
/// - the config loader fails to build
/// - settings failed to load and deserialize into intermediate settings
/// - the intermediate settings fail to finalize into the final settings
pub fn load() -> Result<Settings, SettingsError> {
    let settings_loader = Config::builder()
        // Add in `./Settings.toml`
        // .add_source(config::File::with_name("examples/simple/Settings"))
        .add_source(config::File::with_name("config.yml"))
        // Add in settings from the environment (with a prefix of APP)
        // Eg.. `APP_DEBUG=1 ./target/app` would set the `debug` key
        .add_source(config::Environment::with_prefix("OKH_SCRAPER"))
        .build()?;

    let intermediate_settings = settings_loader
        // .try_deserialize::<HashMap<String, Value>>()
        .try_deserialize::<IntermediateSettings>()?;

    tracing::debug!("{intermediate_settings:#?}");

    intermediate_settings.finalize()
}
