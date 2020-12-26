// Copyright (C) 2020  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

//! This module handles S9 configuration and configuration file parsing
//! TODO: This comment applies to all f64 items, we should switch internal representation to
//! basic units and also consider using a 3rd crate that is aware of physical units:
//!  - freq in `u64` Hz
//!  - voltage in `usize` mV
//!  - temperature to `usize` °C or m°C

use ii_logging::macros::*;

pub mod api;
mod metadata;
pub mod support;

use crate::bm1387::MidstateCount;
use crate::fan;
use crate::hashchain;
use crate::hooks;
use crate::monitor;
use crate::power;

use support::OptionDefault;

use bosminer::client;
use bosminer::hal::{self, BackendConfig as _};

use bosminer_config::{ClientDescriptor, ClientUserInfo};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::fs;
use std::sync::Arc;
use std::time::Duration;

/// Hardware revision
pub const HW_MODEL: &'static str = "Antminer S9";

/// Expected configuration version
const FORMAT_VERSION: &'static str = "1.0";

/// Expected configuration model
pub const FORMAT_MODEL: &'static str = HW_MODEL;

/// Override the default drain channel size as miner tends to burst messages into the logger
pub const ASYNC_LOGGER_DRAIN_CHANNEL_SIZE: usize = 4096;

/// Location of default config
/// TODO: Maybe don't add `.toml` prefix so we could use even JSON
pub const DEFAULT_CONFIG_PATH: &'static str = "/etc/bosminer.toml";

/// Default Hardware ID path
pub const DEFAULT_HW_ID_PATH: &'static str = "/tmp/miner_hwid";

/// Default value for hash chain enabled flag
pub const DEFAULT_HASH_CHAIN_ENABLED: bool = true;

/// Default value for pool enabled flag
pub const DEFAULT_POOL_ENABLED: bool = true;

/// Default number of midstates when AsicBoost is enabled
pub const ASIC_BOOST_MIDSTATE_COUNT: usize = 4;

/// Default number of midstates
pub const DEFAULT_ASIC_BOOST: bool = true;

/// Default PLL frequency for clocking the chips in MHz
pub const DEFAULT_FREQUENCY_MHZ: f64 = 650.0;

/// Default voltage
pub const DEFAULT_VOLTAGE_V: f64 = 8.8;

/// Default temperature control mode
pub const DEFAULT_TEMP_CONTROL_MODE: TempControlMode = TempControlMode::Auto;

/// Default temperatures for temperature control
pub const DEFAULT_TARGET_TEMP_C: f64 = 89.0;
pub const DEFAULT_HOT_TEMP_C: f64 = 100.0;
pub const DEFAULT_DANGEROUS_TEMP_C: f64 = 110.0;

/// Default fan speed for manual target speed
pub const DEFAULT_FAN_SPEED: usize = 100;

/// Default minimal running fans for monitoring
pub const DEFAULT_MIN_FANS: usize = 1;

/// Index of hashboard that is to be instantiated
pub const S9_HASHBOARD_INDEX: usize = 8;

/// Range of hash chain index
pub const HASH_CHAIN_INDEX_MIN: usize = 6;
pub const HASH_CHAIN_INDEX_MAX: usize = 8;

/// Range of PLL frequency for clocking the chips in MHz
pub const FREQUENCY_MHZ_MIN: f64 = 200.0;
pub const FREQUENCY_MHZ_MAX: f64 = 900.0;

/// Range of hash chain voltage
pub const VOLTAGE_V_MIN: f64 = 7.95;
pub const VOLTAGE_V_MAX: f64 = 9.4;

/// Range of monitored temperature
pub const TEMPERATURE_C_MIN: f64 = 0.0;
pub const TEMPERATURE_C_MAX: f64 = 200.0;

/// Range of monitored temperature
pub const FAN_SPEED_MIN: usize = 0;
pub const FAN_SPEED_MAX: usize = 100;

/// Range of possible fans
pub const FANS_MIN: usize = 0;
pub const FANS_MAX: usize = 4;

/// Default ASIC difficulty
pub const DEFAULT_ASIC_DIFFICULTY: usize = 64;

/// Default hashrate interval used for statistics in seconds
pub const DEFAULT_HASHRATE_INTERVAL: Duration = Duration::from_secs(60);

/// Maximum time it takes to compute one job under normal circumstances
pub const JOB_TIMEOUT: Duration = Duration::from_secs(5);

pub struct ResolvedChainConfig {
    pub midstate_count: MidstateCount,
    pub frequency: hashchain::frequency::FrequencySettings,
    pub voltage: power::Voltage,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TempControlMode {
    Auto,
    Manual,
    Disabled,
}

impl std::string::ToString for TempControlMode {
    fn to_string(&self) -> String {
        match self {
            Self::Auto => "auto".to_string(),
            Self::Manual => "manual".to_string(),
            Self::Disabled => "disabled".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Format {
    pub version: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u32>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct HashChainGlobal {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asic_boost: Option<bool>,
    #[serde(flatten)]
    pub overridable: Option<HashChain>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct HashChain {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voltage: Option<f64>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct TempControl {
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<TempControlMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_temp: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hot_temp: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dangerous_temp: Option<f64>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct FanControl {
    #[serde(skip_serializing_if = "Option::is_none")]
    speed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_fans: Option<usize>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(deny_unknown_fields)]
pub struct Backend {
    #[serde(skip)]
    pub info: hal::BackendInfo,
    #[serde(skip)]
    pub client_manager: Option<client::Manager>,
    // TODO: merge pools and clients
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_chain_global: Option<HashChainGlobal>,
    /// We use `BTreeMap` to have alphabetically sorted hash chain indices in persistent
    /// configuration file (TOML)
    #[serde(rename = "hash_chain")]
    #[serde(skip_serializing_if = "Option::is_none")]
    hash_chains: Option<BTreeMap<String, HashChain>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temp_control: Option<TempControl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fan_control: Option<FanControl>,
    #[serde(rename = "group")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<bosminer_config::GroupConfig>>,
    #[serde(skip)]
    pub hooks: Option<Arc<dyn hooks::Hooks>>,
    #[serde(skip)]
    pub fans_on_while_warming_up: Option<bool>,
}

pub trait ConfigBody
where
    Self: Serialize + DeserializeOwned + Default + fmt::Debug,
{
    fn model() -> String;

    fn version() -> String;

    fn version_is_supported(version: &str) -> bool;

    fn sanity_check(&self) -> Result<(), String>;

    fn metadata() -> serde_json::Value;

    fn variant() -> String;
}

#[derive(Debug)]
pub enum FormatWrapperError<B> {
    ParsingError(String),
    IncompatibleFormat(String),
    IncompatibleVersion(String, Option<FormatWrapper<B>>),
    IncorrectBody(String),
}

impl<B> fmt::Display for FormatWrapperError<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParsingError(message) | Self::IncorrectBody(message) => write!(f, "{}", message),
            Self::IncompatibleFormat(model) => write!(f, "incompatible format model '{}'", model),
            Self::IncompatibleVersion(version, _) => {
                write!(f, "incompatible format version '{}'", version)
            }
        }
    }
}

impl<B: fmt::Debug> std::error::Error for FormatWrapperError<B> {}

#[derive(Serialize, Deserialize, Debug)]
pub struct FormatWrapper<B> {
    format: Format,
    #[serde(flatten)]
    pub body: B,
}

impl<B> FormatWrapper<B>
where
    B: ConfigBody,
{
    pub fn sanity_check(&mut self) -> Result<(), FormatWrapperError<B>> {
        // Check compatibility of configuration format
        if self.format.model != B::model() {
            return Err(FormatWrapperError::IncompatibleFormat(
                self.format.model.clone(),
            ));
        }

        self.body
            .sanity_check()
            .map_err(|msg| FormatWrapperError::IncorrectBody(msg))?;

        // Check format version at last to allow caller to treat it as a warning
        if !B::version_is_supported(&self.format.version) {
            return Err(FormatWrapperError::IncompatibleVersion(
                self.format.version.clone(),
                None,
            ));
        }
        Ok(())
    }

    pub fn metadata() -> serde_json::Value {
        // TODO: format-related metadata are for now stored within backend metadata, so move them
        // here and just prepend them to whatever backend returns us
        B::metadata()
    }

    pub fn parse(config_path: &str) -> Result<Self, FormatWrapperError<B>> {
        // Parse config file - either user specified or the default one
        let mut config: Self = bosminer_config::parse(config_path)
            .map_err(|msg| FormatWrapperError::ParsingError(msg))?;

        match config.sanity_check() {
            Ok(_) => Ok(config),
            Err(FormatWrapperError::IncompatibleVersion(version, _)) => Err(
                FormatWrapperError::IncompatibleVersion(version, Some(config)),
            ),
            Err(e) => Err(e),
        }
    }
}

impl Backend {
    pub fn has_groups(&self) -> bool {
        self.groups.as_ref().map(|v| !v.is_empty()).unwrap_or(false)
    }

    pub fn has_pools(&self) -> bool {
        match &self.groups {
            Some(groups) => groups
                .iter()
                .all(|group| group.pools.as_ref().map(|v| !v.is_empty()).unwrap_or(false)),
            None => false,
        }
    }

    pub fn resolve_chain_config(&self, hash_chain_idx: usize) -> ResolvedChainConfig {
        // Take global hash chain configuration or default value
        let overridable = self
            .hash_chain_global
            .as_ref()
            .and_then(|v| v.overridable.as_ref());
        let mut frequency = OptionDefault::new(
            overridable.as_ref().and_then(|v| v.frequency),
            DEFAULT_FREQUENCY_MHZ,
        );
        let mut voltage = OptionDefault::new(
            overridable.as_ref().and_then(|v| v.voltage),
            DEFAULT_VOLTAGE_V,
        );
        let mut enabled = DEFAULT_HASH_CHAIN_ENABLED;

        // If there's a per-chain override then apply it
        if let Some(hash_chain) = self
            .hash_chains
            .as_ref()
            .and_then(|m| m.get(&hash_chain_idx.to_string()))
        {
            enabled = hash_chain.enabled.unwrap_or(enabled);
            frequency = hash_chain
                .frequency
                .map(|v| OptionDefault::Some(v))
                .unwrap_or(frequency);
            voltage = hash_chain
                .voltage
                .map(|v| OptionDefault::Some(v))
                .unwrap_or(voltage);
        }

        // Computed s9-specific values
        ResolvedChainConfig {
            midstate_count: MidstateCount::new(self.midstate_count()),
            frequency: hashchain::frequency::FrequencySettings::from_frequency(
                (*frequency * 1_000_000.0) as usize,
            ),
            // TODO: handle config errors
            voltage: power::Voltage::from_volts(*voltage as f32)
                .expect("TODO: bad voltage requested"),
            enabled,
        }
    }

    pub fn resolve_monitor_config(&self) -> monitor::Config {
        // Get temperature control settings
        let mode = OptionDefault::new(
            self.temp_control.as_ref().and_then(|v| v.mode),
            DEFAULT_TEMP_CONTROL_MODE,
        );
        let target_temp = OptionDefault::new(
            self.temp_control.as_ref().and_then(|v| v.target_temp),
            DEFAULT_TARGET_TEMP_C,
        );
        let hot_temp = OptionDefault::new(
            self.temp_control.as_ref().and_then(|v| v.hot_temp),
            DEFAULT_HOT_TEMP_C,
        );
        let dangerous_temp = OptionDefault::new(
            self.temp_control.as_ref().and_then(|v| v.dangerous_temp),
            DEFAULT_DANGEROUS_TEMP_C,
        );

        // Get fan control settings
        let fan_speed = OptionDefault::new(
            self.fan_control.as_ref().and_then(|v| v.speed),
            DEFAULT_FAN_SPEED,
        );
        let min_fans = OptionDefault::new(
            self.fan_control.as_ref().and_then(|v| v.min_fans),
            DEFAULT_MIN_FANS,
        );

        let temp_config;
        let fan_config;

        // Configure temperature controller
        match *mode {
            TempControlMode::Auto | TempControlMode::Manual => {
                temp_config = Some(monitor::TempControlConfig {
                    dangerous_temp: *dangerous_temp as f32,
                    hot_temp: *hot_temp as f32,
                });
            }
            TempControlMode::Disabled => {
                temp_config = None;
                // do sanity checks
                if hot_temp.is_some() {
                    warn!(
                        "Unused 'hot_temp' ({}) because 'disable' mode is set",
                        *hot_temp
                    );
                }
                if dangerous_temp.is_some() {
                    warn!(
                        "Unused 'dangerous_temp' ({}) because 'disable' mode is set",
                        *hot_temp
                    );
                }
            }
        };

        // Configure fan controller
        match *mode {
            TempControlMode::Auto => {
                fan_config = Some(monitor::FanControlConfig {
                    mode: monitor::FanControlMode::TargetTemperature(*target_temp as f32),
                    min_fans: *min_fans,
                });
                // do sanity checks
                if fan_speed.is_some() {
                    warn!(
                        "Unused fan 'speed' ({}) because 'auto' mode is set",
                        *fan_speed
                    );
                }
            }
            TempControlMode::Manual | TempControlMode::Disabled => {
                fan_config = if fan_speed.eq_some(&0) && min_fans.eq_some(&0) {
                    // completely disable fan controller when all settings are set to 0
                    None
                } else {
                    Some(monitor::FanControlConfig {
                        mode: monitor::FanControlMode::FixedSpeed(fan::Speed::new(*fan_speed)),
                        min_fans: *min_fans,
                    })
                };
                // do sanity checks
                if target_temp.is_some() {
                    warn!(
                        "Unused 'target_temp' ({}) because 'auto' mode is not set",
                        *fan_speed
                    );
                }
            }
        };

        monitor::Config {
            temp_config,
            fan_config,
            fans_on_while_warming_up: self.fans_on_while_warming_up.unwrap_or(true),
        }
    }

    pub fn fill_info<T>(&mut self) -> Result<(), std::io::Error>
    where
        T: ConfigBody,
    {
        self.info.hw_rev = HW_MODEL.to_string();
        self.info.dev_id = fs::read_to_string(DEFAULT_HW_ID_PATH)?.trim().to_string();
        self.info.fw_ver = format!("{} {}", T::variant(), bosminer::version::STRING.to_string());
        Ok(())
    }
}

impl ConfigBody for Backend {
    fn model() -> String {
        return FORMAT_MODEL.into();
    }

    fn version() -> String {
        return FORMAT_VERSION.into();
    }

    fn version_is_supported(version: &str) -> bool {
        version == FORMAT_VERSION
    }

    fn sanity_check(&self) -> Result<(), String> {
        // Check if all hash chain keys have meaningful name
        if let Some(hash_chains) = &self.hash_chains {
            for idx in hash_chains.keys() {
                let _ = idx
                    .parse::<usize>()
                    .map_err(|_| format!("hash chain index '{}' is not number", idx))
                    .and_then(|idx| {
                        if (HASH_CHAIN_INDEX_MIN..=HASH_CHAIN_INDEX_MAX).contains(&idx) {
                            Ok(idx)
                        } else {
                            Err(format!(
                                "hash chain index '{}' is out of range '{}..{}'",
                                idx, HASH_CHAIN_INDEX_MIN, HASH_CHAIN_INDEX_MAX
                            ))
                        }
                    })?;
            }
        }

        // Analyze group configuration, make sure the groups are unique, and build descriptor
        // topology out of the configuration data
        // Don't worry if is this section missing, maybe there are some pools on command line
        if let Some(groups) = &self.groups {
            let mut group_names = HashSet::with_capacity(groups.len());
            for group in groups {
                if let Some(name) = group_names.replace(&group.descriptor.name) {
                    Err(format!("group with name '{}' already defined", name))?;
                }
                if let Some(pools) = &group.pools {
                    for pool in pools {
                        let _ = ClientDescriptor::create(
                            pool.url.as_str(),
                            &ClientUserInfo::new(pool.user.as_str(), pool.password.as_deref()),
                            pool.enabled.unwrap_or(DEFAULT_POOL_ENABLED),
                        )
                        .map_err(|e| {
                            format!("{} in pool '{}@{}'", e.to_string(), pool.url, pool.user)
                        })?;
                    }
                }
            }
        }

        Ok(())
    }

    fn metadata() -> serde_json::Value {
        metadata::for_backend()
    }

    fn variant() -> String {
        bosminer::SIGNATURE.into()
    }
}

impl hal::BackendConfig for Backend {
    #[inline]
    fn midstate_count(&self) -> usize {
        if self
            .hash_chain_global
            .as_ref()
            .and_then(|v| v.asic_boost)
            .unwrap_or(DEFAULT_ASIC_BOOST)
        {
            ASIC_BOOST_MIDSTATE_COUNT
        } else {
            1
        }
    }

    fn set_client_manager(&mut self, client_manager: client::Manager) {
        self.client_manager.replace(client_manager);
    }

    fn info(&self) -> Option<hal::BackendInfo> {
        Some(self.info.clone())
    }
}
