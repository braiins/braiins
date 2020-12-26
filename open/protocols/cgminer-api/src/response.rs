// Copyright (C) 2019  Braiins Systems s.r.o.
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

//! Defines all the CGMiner API responses

pub mod ext;

use crate::support;

use serde::{Serialize, Serializer};
use serde_json as json;

pub type Time = u32;
pub type Elapsed = u64;
pub type Interval = f64;
pub type Percent = f64;
pub type Difficulty = f64;
pub type MegaHashes = f64;
pub type GigaHashes = MegaHashes;
pub type TotalMegaHashes = f64;
pub type Utility = f64;
pub type Temperature = f64;

#[allow(dead_code)]
/// CGMiner API Status indicator.
/// (warning and info levels not currently used.)
#[derive(Serialize, Eq, PartialEq, Copy, Clone, Debug)]
pub enum Status {
    W,
    I,
    S,
    E,
}

#[allow(dead_code)]
#[derive(Serialize, Eq, PartialEq, Copy, Clone, Debug)]
pub enum Bool {
    N,
    Y,
}

impl<T> From<Option<T>> for Bool {
    fn from(value: Option<T>) -> Self {
        match value {
            None => Bool::N,
            Some(_) => Bool::Y,
        }
    }
}

#[allow(dead_code)]
#[derive(Serialize, Eq, PartialEq, Clone, Debug)]
#[serde(rename_all = "PascalCase")]
pub enum PoolStatus {
    Disabled,
    Rejecting,
    Dead,
    Alive,
    Unknown,
}

#[allow(dead_code)]
#[derive(Serialize, Eq, PartialEq, Clone, Debug)]
#[serde(rename_all = "PascalCase")]
pub enum AscStatus {
    Alive,
    Sick,
    Dead,
    NoStart,
    Initialising,
    Unknown,
}

#[allow(dead_code)]
#[derive(Serialize, Eq, PartialEq, Clone, Debug)]
#[serde(rename_all = "PascalCase")]
pub enum MultipoolStrategy {
    Failover,
    #[serde(rename = "Round Robin")]
    RoundRobin,
    Rotate,
    #[serde(rename = "Load Balance")]
    LoadBalance,
    Balance,
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
#[repr(u32)]
pub enum StatusCode {
    // command status codes
    Pool = 7,
    Devs = 9,
    Summary = 11,
    Version = 22,
    SwitchPool = 27,
    MineConfig = 33,
    EnablePool = 47,
    DisablePool = 48,
    AddPool = 55,
    RemovePool = 68,
    DevDetails = 69,
    Stats = 70,
    Check = 72,
    Coin = 78,
    AscCount = 104,
    Asc = 106,
    Lcd = 125,

    // extended command status codes
    TempCtrl = 200,
    Temps = 201,
    Fans = 202,
    TunerStatus = 203,

    // info status codes
    PoolAlreadyEnabled = 49,
    PoolAlreadyDisabled = 50,

    // error status codes
    InvalidCommand = 14,
    MissingAscParameter = 15,
    InvalidJSON = 23,
    MissingCommand = 24,
    MissingPoolParameter = 25,
    InvalidPoolId = 26,
    AccessDeniedCmd = 45,
    MissingAddPoolDetails = 52,
    InvalidAddPoolDetails = 53,
    MissingCheckCmd = 71,
    InvalidAscId = 107,

    // special value which is added to the custom status codes
    CustomBase = 300,
}

/// Holds standard protocol status code or a custom one. Unifying these 2 variants allows
/// adding custom status codes to the API.
#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum StatusCodeType {
    Protocol(StatusCode),
    Custom(u32),
}

impl From<StatusCode> for StatusCodeType {
    fn from(code: StatusCode) -> Self {
        StatusCodeType::Protocol(code)
    }
}

impl Serialize for StatusCodeType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let code = match self {
            StatusCodeType::Protocol(code) => *code as u32,
            StatusCodeType::Custom(code) => StatusCode::CustomBase as u32 + *code,
        };
        serializer.serialize_u32(code)
    }
}

pub enum InfoCode {
    PoolAlreadyEnabled(i32, String),
    PoolAlreadyDisabled(i32, String),
}

impl From<InfoCode> for Dispatch {
    fn from(code: InfoCode) -> Self {
        Error::from(code).into()
    }
}

pub enum ErrorCode {
    InvalidCommand,
    MissingAscParameter,
    InvalidJSON,
    MissingCommand,
    MissingPoolParameter,
    InvalidPoolId(i32, i32),
    AccessDeniedCmd(String),
    MissingAddPoolDetails,
    InvalidAddPoolDetails(String),
    MissingCheckCmd,
    InvalidAscId(i32, i32),
}

impl From<ErrorCode> for Dispatch {
    fn from(code: ErrorCode) -> Self {
        Error::from(code).into()
    }
}

pub struct Error {
    status: Status,
    code: StatusCodeType,
    msg: String,
}

impl Error {
    #[inline]
    pub fn msg(&self) -> &String {
        &self.msg
    }

    #[inline]
    pub fn from_custom_error<T>(code: T, msg: String) -> Self
    where
        T: Into<u32>,
    {
        Self {
            status: Status::E,
            code: StatusCodeType::Custom(code.into()),
            msg,
        }
    }
}

impl From<InfoCode> for Error {
    fn from(code: InfoCode) -> Self {
        let (code, msg) = match code {
            InfoCode::PoolAlreadyEnabled(idx, url) => (
                StatusCode::PoolAlreadyEnabled,
                format!("Pool {}:'{}' already enabled", idx, url),
            ),
            InfoCode::PoolAlreadyDisabled(idx, url) => (
                StatusCode::PoolAlreadyDisabled,
                format!("Pool {}:'{}' already disabled", idx, url),
            ),
        };

        Self {
            status: Status::I,
            code: code.into(),
            msg,
        }
    }
}

impl From<ErrorCode> for Error {
    fn from(code: ErrorCode) -> Self {
        let (code, msg) = match code {
            ErrorCode::InvalidCommand => {
                (StatusCode::InvalidCommand, "Invalid command".to_string())
            }
            ErrorCode::MissingAscParameter => (
                StatusCode::MissingAscParameter,
                "Missing device id parameter".to_string(),
            ),
            ErrorCode::InvalidJSON => (StatusCode::InvalidJSON, "Invalid JSON".to_string()),
            ErrorCode::MissingCommand => (
                StatusCode::MissingCommand,
                "Missing JSON 'command'".to_string(),
            ),
            ErrorCode::MissingPoolParameter => (
                StatusCode::MissingPoolParameter,
                "Missing pool id parameter".to_string(),
            ),
            ErrorCode::InvalidPoolId(idx_requested, idx_last) => (
                StatusCode::InvalidAscId,
                format!(
                    "Invalid pool id {} - range is 0 - {}",
                    idx_requested, idx_last
                ),
            ),
            ErrorCode::AccessDeniedCmd(name) => (
                StatusCode::AccessDeniedCmd,
                format!("Access denied to '{}' command", name),
            ),
            ErrorCode::MissingAddPoolDetails => (
                StatusCode::MissingAddPoolDetails,
                "Missing addpool details".to_string(),
            ),
            ErrorCode::InvalidAddPoolDetails(parameter) => (
                StatusCode::InvalidAddPoolDetails,
                format!("Invalid addpool details '{}'", parameter),
            ),
            ErrorCode::MissingCheckCmd => {
                (StatusCode::MissingCheckCmd, "Missing check cmd".to_string())
            }
            ErrorCode::InvalidAscId(idx_requested, idx_last) => (
                StatusCode::InvalidAscId,
                format!(
                    "Invalid ASC id {} - range is 0 - {}",
                    idx_requested, idx_last
                ),
            ),
        };

        Self {
            status: Status::E,
            code: code.into(),
            msg,
        }
    }
}

impl From<Error> for Dispatch {
    fn from(error: Error) -> Self {
        Self {
            status: error.status,
            code: error.code,
            msg: error.msg().clone(),
            body: None,
        }
    }
}

/// STATUS structure present in all replies
#[derive(Serialize, PartialEq, Clone, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct StatusInfo {
    #[serde(rename = "STATUS")]
    pub status: Status,
    pub when: Time,
    pub code: StatusCodeType,
    pub msg: String,
    pub description: String,
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Pool {
    #[serde(rename = "POOL")]
    pub idx: i32,
    #[serde(rename = "URL")]
    pub url: String,
    #[serde(rename = "Status")]
    pub status: PoolStatus,
    #[serde(rename = "Priority")]
    pub priority: i32,
    #[serde(rename = "Quota")]
    pub quota: i32,
    #[serde(rename = "Long Poll")]
    pub long_poll: Bool,
    #[serde(rename = "Getworks")]
    pub getworks: u32,
    #[serde(rename = "Accepted")]
    pub accepted: u64,
    #[serde(rename = "Rejected")]
    pub rejected: u64,
    #[serde(rename = "Works")]
    pub works: i32,
    #[serde(rename = "Discarded")]
    pub discarded: u32,
    #[serde(rename = "Stale")]
    pub stale: u32,
    #[serde(rename = "Get Failures")]
    pub get_failures: u32,
    #[serde(rename = "Remote Failures")]
    pub remote_failures: u32,
    #[serde(rename = "User")]
    pub user: String,
    #[serde(rename = "Last Share Time")]
    pub last_share_time: Time,
    #[serde(rename = "Diff1 Shares")]
    pub diff1_shares: u64,
    #[serde(rename = "Proxy Type")]
    pub proxy_type: String,
    #[serde(rename = "Proxy")]
    pub proxy: String,
    #[serde(rename = "Difficulty Accepted")]
    pub difficulty_accepted: Difficulty,
    #[serde(rename = "Difficulty Rejected")]
    pub difficulty_rejected: Difficulty,
    #[serde(rename = "Difficulty Stale")]
    pub difficulty_stale: Difficulty,
    #[serde(rename = "Last Share Difficulty")]
    pub last_share_difficulty: Difficulty,
    #[serde(rename = "Work Difficulty")]
    pub work_difficulty: Difficulty,
    #[serde(rename = "Has Stratum")]
    pub has_stratum: bool,
    #[serde(rename = "Stratum Active")]
    pub stratum_active: bool,
    #[serde(rename = "Stratum URL")]
    pub stratum_url: String,
    #[serde(rename = "Stratum Difficulty")]
    pub stratum_difficulty: Difficulty,
    #[serde(rename = "Has Vmask")]
    pub has_vmask: bool,
    #[serde(rename = "Has GBT")]
    pub has_gbt: bool,
    #[serde(rename = "Best Share")]
    pub best_share: u64,
    #[serde(rename = "Pool Rejected%")]
    pub pool_rejected_ratio: Percent,
    #[serde(rename = "Pool Stale%")]
    pub pool_stale_ratio: Percent,
    #[serde(rename = "Bad Work")]
    pub bad_work: u64,
    #[serde(rename = "Current Block Height")]
    pub current_block_height: u32,
    #[serde(rename = "Current Block Version")]
    pub current_block_version: u32,
    // Follows attribute extensions
    #[serde(rename = "AsicBoost")]
    pub asic_boost: bool,
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Pools {
    pub list: Vec<Pool>,
}

impl From<Pools> for Dispatch {
    fn from(pools: Pools) -> Self {
        let pool_count = pools.list.len();
        Dispatch::from_success(
            StatusCode::Pool.into(),
            format!("{} Pool(s)", pool_count),
            Some(Body {
                name: "POOLS",
                list: pools.list,
            }),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Asc {
    #[serde(rename = "ASC")]
    pub idx: i32,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "ID")]
    pub id: i32,
    #[serde(rename = "Enabled")]
    pub enabled: Bool,
    #[serde(rename = "Status")]
    pub status: AscStatus,
    #[serde(rename = "Temperature")]
    pub temperature: Temperature,
    #[serde(rename = "MHS av")]
    pub mhs_av: MegaHashes,
    #[serde(rename = "MHS 5s")]
    pub mhs_5s: MegaHashes,
    #[serde(rename = "MHS 1m")]
    pub mhs_1m: MegaHashes,
    #[serde(rename = "MHS 5m")]
    pub mhs_5m: MegaHashes,
    #[serde(rename = "MHS 15m")]
    pub mhs_15m: MegaHashes,
    #[serde(rename = "Accepted")]
    pub accepted: i32,
    #[serde(rename = "Rejected")]
    pub rejected: i32,
    #[serde(rename = "Hardware Errors")]
    pub hardware_errors: i32,
    #[serde(rename = "Utility")]
    pub utility: Utility,
    #[serde(rename = "Last Share Pool")]
    pub last_share_pool: i32,
    #[serde(rename = "Last Share Time")]
    pub last_share_time: Time,
    #[serde(rename = "Total MH")]
    pub total_mega_hashes: TotalMegaHashes,
    #[serde(rename = "Diff1 Work")]
    pub diff1_work: u64,
    #[serde(rename = "Difficulty Accepted")]
    pub difficulty_accepted: Difficulty,
    #[serde(rename = "Difficulty Rejected")]
    pub difficulty_rejected: Difficulty,
    #[serde(rename = "Last Share Difficulty")]
    pub last_share_difficulty: Difficulty,
    #[serde(rename = "Last Valid Work")]
    pub last_valid_work: Time,
    #[serde(rename = "Device Hardware%")]
    pub device_hardware_ratio: Percent,
    #[serde(rename = "Device Rejected%")]
    pub device_rejected_ratio: Percent,
    #[serde(rename = "Device Elapsed")]
    pub device_elapsed: Elapsed,
    // Follows attribute extensions
    #[serde(rename = "Hardware Error MHS 15m")]
    pub hardware_error_mhs_15m: MegaHashes,
    #[serde(rename = "Nominal MHS")]
    pub nominal_mhs: MegaHashes,
}

impl From<Asc> for Dispatch {
    fn from(asc: Asc) -> Self {
        let idx = asc.idx;
        Dispatch::from_success(
            StatusCode::Asc.into(),
            format!("ASC{}", idx),
            Some(Body {
                name: "ASC",
                list: vec![asc],
            }),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Devs {
    pub list: Vec<Asc>,
}

impl From<Devs> for Dispatch {
    fn from(devs: Devs) -> Self {
        let asc_count = devs.list.len();
        Dispatch::from_success(
            StatusCode::Devs.into(),
            format!("{} ASC(s)", asc_count),
            Some(Body {
                name: "DEVS",
                list: devs.list,
            }),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Summary {
    #[serde(rename = "Elapsed")]
    pub elapsed: Elapsed,
    #[serde(rename = "MHS av")]
    pub mhs_av: MegaHashes,
    #[serde(rename = "MHS 5s")]
    pub mhs_5s: MegaHashes,
    #[serde(rename = "MHS 1m")]
    pub mhs_1m: MegaHashes,
    #[serde(rename = "MHS 5m")]
    pub mhs_5m: MegaHashes,
    #[serde(rename = "MHS 15m")]
    pub mhs_15m: MegaHashes,
    #[serde(rename = "Found Blocks")]
    pub found_blocks: u32,
    #[serde(rename = "Getworks")]
    pub getworks: u64,
    #[serde(rename = "Accepted")]
    pub accepted: u64,
    #[serde(rename = "Rejected")]
    pub rejected: u64,
    #[serde(rename = "Hardware Errors")]
    pub hardware_errors: i32,
    #[serde(rename = "Utility")]
    pub utility: Utility,
    #[serde(rename = "Discarded")]
    pub discarded: i64,
    #[serde(rename = "Stale")]
    pub stale: u64,
    #[serde(rename = "Get Failures")]
    pub get_failures: u32,
    #[serde(rename = "Local Work")]
    pub local_work: u32,
    #[serde(rename = "Remote Failures")]
    pub remote_failures: u32,
    #[serde(rename = "Network Blocks")]
    pub network_blocks: u32,
    #[serde(rename = "Total MH")]
    pub total_mega_hashes: TotalMegaHashes,
    #[serde(rename = "Work Utility")]
    pub work_utility: Utility,
    #[serde(rename = "Difficulty Accepted")]
    pub difficulty_accepted: Difficulty,
    #[serde(rename = "Difficulty Rejected")]
    pub difficulty_rejected: Difficulty,
    #[serde(rename = "Difficulty Stale")]
    pub difficulty_stale: Difficulty,
    #[serde(rename = "Best Share")]
    pub best_share: u64,
    #[serde(rename = "Device Hardware%")]
    pub device_hardware_ratio: Percent,
    #[serde(rename = "Device Rejected%")]
    pub device_rejected_ratio: Percent,
    #[serde(rename = "Pool Rejected%")]
    pub pool_rejected_ratio: Percent,
    #[serde(rename = "Pool Stale%")]
    pub pool_stale_ratio: Percent,
    #[serde(rename = "Last getwork")]
    pub last_getwork: Time,
    // Follows attribute extensions
    #[serde(rename = "MHS 24h")]
    pub mhs_24h: MegaHashes,
}

impl From<Summary> for Dispatch {
    fn from(summary: Summary) -> Self {
        Dispatch::from_success(
            StatusCode::Summary.into(),
            "Summary".to_string(),
            Some(Body {
                name: "SUMMARY",
                list: vec![summary],
            }),
        )
    }
}

#[derive(PartialEq, Clone, Debug)]
pub(crate) struct Version {
    pub signature: String,
    pub miner: String,
    pub api: String,
}

impl Serialize for Version {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry(&self.signature, &self.miner)?;
        map.serialize_entry("API", &self.api)?;
        map.end()
    }
}

impl From<Version> for Dispatch {
    fn from(version: Version) -> Self {
        Dispatch::from_success(
            StatusCode::Version.into(),
            format!("{} versions", crate::SIGNATURE_TAG),
            Some(Body {
                name: "VERSION",
                list: vec![version],
            }),
        )
    }
}

pub struct SwitchPool {
    pub idx: usize,
    pub url: String,
}

impl From<SwitchPool> for Dispatch {
    fn from(switch_pool: SwitchPool) -> Self {
        Dispatch::from_success::<()>(
            StatusCode::SwitchPool.into(),
            format!(
                "Switching to pool {}: '{}'",
                switch_pool.idx, switch_pool.url
            ),
            None,
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Config {
    #[serde(rename = "ASC Count")]
    pub asc_count: i32,
    #[serde(rename = "PGA Count")]
    pub pga_count: i32,
    #[serde(rename = "Pool Count")]
    pub pool_count: i32,
    #[serde(rename = "Strategy")]
    pub strategy: MultipoolStrategy,
    #[serde(rename = "Log Interval")]
    pub log_interval: i32,
    #[serde(rename = "Device Code")]
    pub device_code: String,
    #[serde(rename = "OS")]
    pub os: String,
    #[serde(rename = "Hotplug")]
    pub hotplug: String,
}

impl From<Config> for Dispatch {
    fn from(config: Config) -> Self {
        Dispatch::from_success(
            StatusCode::MineConfig.into(),
            format!("{} config", crate::SIGNATURE_TAG),
            Some(Body {
                name: "CONFIG",
                list: vec![config],
            }),
        )
    }
}

pub struct EnablePool {
    pub idx: usize,
    pub url: String,
}

impl From<EnablePool> for Dispatch {
    fn from(enable_pool: EnablePool) -> Self {
        Dispatch::from_success::<()>(
            StatusCode::EnablePool.into(),
            format!("Enabling pool {}:'{}'", enable_pool.idx, enable_pool.url),
            None,
        )
    }
}

pub struct DisablePool {
    pub idx: usize,
    pub url: String,
}

impl From<DisablePool> for Dispatch {
    fn from(disable_pool: DisablePool) -> Self {
        Dispatch::from_success::<()>(
            StatusCode::DisablePool.into(),
            format!("Disabling pool {}:'{}'", disable_pool.idx, disable_pool.url),
            None,
        )
    }
}

pub struct AddPool {
    pub idx: usize,
    pub url: String,
}

impl From<AddPool> for Dispatch {
    fn from(add_pool: AddPool) -> Self {
        Dispatch::from_success::<()>(
            StatusCode::AddPool.into(),
            format!("Added pool {}: '{}'", add_pool.idx, add_pool.url),
            None,
        )
    }
}

pub struct RemovePool {
    pub idx: usize,
    pub url: String,
}

impl From<RemovePool> for Dispatch {
    fn from(remove_pool: RemovePool) -> Self {
        Dispatch::from_success::<()>(
            StatusCode::RemovePool.into(),
            format!("Removed pool {}:'{}'", remove_pool.idx, remove_pool.url),
            None,
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct DevDetail<T> {
    #[serde(rename = "DEVDETAILS")]
    pub idx: i32,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "ID")]
    pub id: i32,
    #[serde(rename = "Driver")]
    pub driver: String,
    #[serde(rename = "Kernel")]
    pub kernel: String,
    #[serde(rename = "Model")]
    pub model: String,
    #[serde(rename = "Device Path")]
    pub device_path: String,
    #[serde(flatten)]
    pub info: T,
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct DevDetails<T> {
    pub list: Vec<DevDetail<T>>,
}

impl<T> From<DevDetails<T>> for Dispatch
where
    T: serde::Serialize,
{
    fn from(dev_details: DevDetails<T>) -> Self {
        Dispatch::from_success(
            StatusCode::DevDetails.into(),
            "Device Details".to_string(),
            Some(Body {
                name: "DEVDETAILS",
                list: dev_details.list,
            }),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct PoolStats {
    #[serde(flatten)]
    pub header: StatsHeader,
    #[serde(rename = "Pool Calls")]
    pub pool_calls: u32,
    #[serde(rename = "Pool Attempts")]
    pub pool_attempts: u32,
    #[serde(rename = "Pool Wait")]
    pub pool_wait: Interval,
    #[serde(rename = "Pool Max")]
    pub pool_max: Interval,
    #[serde(rename = "Pool Min")]
    pub pool_min: Interval,
    #[serde(rename = "Pool Av")]
    pub pool_av: f64,
    #[serde(rename = "Work Had Roll Time")]
    pub work_had_roll_time: bool,
    #[serde(rename = "Work Can Roll")]
    pub work_can_roll: bool,
    #[serde(rename = "Work Had Expire")]
    pub work_had_expire: bool,
    #[serde(rename = "Work Roll Time")]
    pub work_roll_time: u32,
    #[serde(rename = "Work Diff")]
    pub work_diff: Difficulty,
    #[serde(rename = "Min Diff")]
    pub min_diff: Difficulty,
    #[serde(rename = "Max Diff")]
    pub max_diff: Difficulty,
    #[serde(rename = "Min Diff Count")]
    pub min_diff_count: u32,
    #[serde(rename = "Max Diff Count")]
    pub max_diff_count: u32,
    #[serde(rename = "Times Sent")]
    pub times_sent: u64,
    #[serde(rename = "Bytes Sent")]
    pub bytes_sent: u64,
    #[serde(rename = "Times Recv")]
    pub times_recv: u64,
    #[serde(rename = "Bytes Recv")]
    pub bytes_recv: u64,
    #[serde(rename = "Net Bytes Sent")]
    pub net_bytes_sent: u64,
    #[serde(rename = "Net Bytes Recv")]
    pub net_bytes_recv: u64,
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct AscStats {
    #[serde(flatten)]
    pub header: StatsHeader,
}

#[derive(Serialize, PartialEq, Clone, Debug)]
#[serde(untagged)]
enum StatsType {
    Pool(PoolStats),
    Asc(AscStats),
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct StatsHeader {
    #[serde(rename = "STATS")]
    pub idx: i32,
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Elapsed")]
    pub elapsed: Elapsed,
    #[serde(rename = "Calls")]
    pub calls: u32,
    #[serde(rename = "Wait")]
    pub wait: Interval,
    #[serde(rename = "Max")]
    pub max: Interval,
    #[serde(rename = "Min")]
    pub min: Interval,
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Stats {
    pub asc_stats: Vec<AscStats>,
    pub pool_stats: Vec<PoolStats>,
}

impl Stats {
    fn into_list(self) -> Vec<StatsType> {
        self.asc_stats
            .into_iter()
            .map(|stats| StatsType::Asc(stats))
            .chain(
                self.pool_stats
                    .into_iter()
                    .map(|stats| StatsType::Pool(stats)),
            )
            .collect()
    }
}

impl From<Stats> for Dispatch {
    fn from(stats: Stats) -> Self {
        Dispatch::from_success(
            StatusCode::Stats.into(),
            format!("{} stats", crate::SIGNATURE_TAG),
            Some(Body {
                name: "STATS",
                list: stats.into_list(),
            }),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub(crate) struct Check {
    #[serde(rename = "Exists")]
    pub exists: Bool,
    #[serde(rename = "Access")]
    pub access: Bool,
}

impl From<Check> for Dispatch {
    fn from(check: Check) -> Self {
        Dispatch::from_success(
            StatusCode::Check.into(),
            "Check command".to_string(),
            Some(Body {
                name: "CHECK",
                list: vec![check],
            }),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Coin {
    #[serde(rename = "Hash Method")]
    pub hash_method: String,
    #[serde(rename = "Current Block Time")]
    pub current_block_time: Interval,
    #[serde(rename = "Current Block Hash")]
    pub current_block_hash: String,
    #[serde(rename = "LP")]
    pub lp: bool,
    #[serde(rename = "Network Difficulty")]
    pub network_difficulty: Difficulty,
}

impl From<Coin> for Dispatch {
    fn from(coin: Coin) -> Self {
        Dispatch::from_success(
            StatusCode::Coin.into(),
            format!("{} coin", crate::SIGNATURE_TAG),
            Some(Body {
                name: "COIN",
                list: vec![coin],
            }),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct AscCount {
    #[serde(rename = "Count")]
    pub count: i32,
}

impl From<AscCount> for Dispatch {
    fn from(asc_count: AscCount) -> Self {
        Dispatch::from_success(
            StatusCode::AscCount.into(),
            "ASC count".to_string(),
            Some(Body {
                name: "ASCS",
                list: vec![asc_count],
            }),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Lcd {
    #[serde(rename = "Elapsed")]
    pub elapsed: Elapsed,
    #[serde(rename = "GHS av")]
    pub ghs_av: GigaHashes,
    #[serde(rename = "GHS 5m")]
    pub ghs_5m: GigaHashes,
    #[serde(rename = "GHS 5s")]
    pub ghs_5s: GigaHashes,
    #[serde(rename = "Temperature")]
    pub temperature: Temperature,
    #[serde(rename = "Last Share Difficulty")]
    pub last_share_difficulty: Difficulty,
    #[serde(rename = "Last Share Time")]
    pub last_share_time: Time,
    #[serde(rename = "Best Share")]
    pub best_share: u64,
    #[serde(rename = "Last Valid Work")]
    pub last_valid_work: Time,
    #[serde(rename = "Found Blocks")]
    pub found_blocks: u32,
    #[serde(rename = "Current Pool")]
    pub current_pool: String,
    #[serde(rename = "User")]
    pub user: String,
}

impl From<Lcd> for Dispatch {
    fn from(lcd: Lcd) -> Self {
        Dispatch::from_success(
            StatusCode::Lcd.into(),
            "LCD".to_string(),
            Some(Body {
                name: "LCD",
                list: vec![lcd],
            }),
        )
    }
}

pub struct Body<S: Serialize> {
    pub name: &'static str,
    pub list: Vec<S>,
}

/// Generic container for any response, ensures conforming serialization
#[derive(Debug)]
pub struct Dispatch {
    status: Status,
    code: StatusCodeType,
    msg: String,
    body: Option<(&'static str, json::Value)>,
}

impl Dispatch {
    fn from_success<S: Serialize>(
        code: StatusCodeType,
        msg: String,
        body: Option<Body<S>>,
    ) -> Self {
        let body = body.map(|body| {
            (
                body.name,
                json::to_value(body.list).expect("BUG: response serialization failed"),
            )
        });

        Self {
            status: Status::S,
            code,
            msg,
            body,
        }
    }

    pub fn from_custom_success<S, T>(code: T, msg: String, body: Option<Body<S>>) -> Self
    where
        S: Serialize,
        T: Into<u32>,
    {
        Self::from_success(StatusCodeType::Custom(code.into()), msg, body)
    }

    fn create_status_info(
        &self,
        when: Time,
        signature: &String,
        description: &String,
    ) -> StatusInfo {
        StatusInfo {
            status: self.status,
            when,
            code: self.code,
            msg: self.msg.replace(crate::SIGNATURE_TAG, signature.as_str()),
            description: description.clone(),
        }
    }

    pub fn into_response(
        self,
        when: Time,
        signature: &String,
        description: &String,
    ) -> support::SingleResponse {
        support::SingleResponse {
            status_info: self.create_status_info(when, signature, description),
            body: self.body,
        }
    }
}
