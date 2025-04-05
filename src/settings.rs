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

const USER_AGENT_DEFAULT: &str = "okh-scraper github.com/iop-alliance/OpenKnowHow";

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
    pub user_agent: Option<String>,
    pub database: Database,
    pub scrapers: HashMap<String, HashMap<String, Value>>,
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
    pub scrapers: HashMap<String, Box<dyn Scraper>>,
}

impl IntermediateSettings {
    #[must_use]
    pub fn partial(&self) -> PartialSettings {
        PartialSettings {
            user_agent: self
                .user_agent
                .clone()
                .unwrap_or_else(|| USER_AGENT_DEFAULT.to_owned()),
            database: self.database.clone(),
        }
    }

    pub fn finalize(self) -> Result<Settings, SettingsError> {
        let scraper_factories = scrapers::assemble_factories();
        let mut scrapers = HashMap::new();
        let config_partial = Arc::new(self.partial());
        for (scraper_id, properties) in self.scrapers {
            let scraper_type = properties
                .get("scraper_type")
                .expect("scraper section requires property 'scraper_type'")
                .as_str()
                .expect("property 'scraper_type' needs to be a string");
            tracing::debug!("Scraper '{scraper_id}' has type: '{scraper_type}' - parsing ...");
            if [
                "none",
                "appropedia",
                "forgejo",
                "github",
                "gitlab",
                "manifests-repo",
                "oshwa",
                // "thingiverse",
            ]
            .contains(&scraper_type)
            {
                // TODO HACK
                tracing::debug!(
                    "ignoring '{scraper_id}' because type not yet implemented: '{scraper_type}'"
                );
                continue;
            }
            let factory = scraper_factories
                .get(scraper_type)
                .unwrap_or_else(|| panic!("No scraper found for type '{scraper_type}'"));
            let scraper = factory.create(
                Arc::<PartialSettings>::clone(&config_partial),
                properties
                    .get("config")
                    .unwrap_or_else(|| panic!("No config found for scraper '{scraper_id}'"))
                    .clone(),
            )?;
            scrapers.insert(scraper_id, scraper);
        }

        Ok(Settings {
            user_agent: self
                .user_agent
                .unwrap_or_else(|| USER_AGENT_DEFAULT.to_owned()),
            database: self.database,
            scrapers,
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
