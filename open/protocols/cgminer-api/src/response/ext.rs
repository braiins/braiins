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

//! Defines all extended BOSminer API responses

use super::*;

#[derive(Serialize, PartialEq, Clone, Debug)]
#[serde(rename_all = "PascalCase")]
pub enum TempCtrlMode {
    Automatic,
    Manual,
    Disabled,
}

/// Basic temperature control settings
#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct TempCtrl {
    #[serde(rename = "Mode")]
    pub mode: TempCtrlMode,
    /// Temperature setpoint
    #[serde(rename = "Target")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<f32>,
    /// Hot temperature threshold is typically intended to warn the user
    #[serde(rename = "Hot")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hot: Option<f32>,
    /// Dangerous temperature is recommended to result in shutdown to prevent hardware damage
    /// from overheating
    #[serde(rename = "Dangerous")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dangerous: Option<f32>,
}

impl From<TempCtrl> for Dispatch {
    fn from(temp_ctrl: TempCtrl) -> Self {
        Dispatch::from_success(
            StatusCode::TempCtrl.into(),
            "Temperature control".to_string(),
            Some(Body {
                name: "TEMPCTRL",
                list: vec![temp_ctrl],
            }),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Temp<T> {
    #[serde(rename = "TEMP")]
    pub idx: i32,
    #[serde(rename = "ID")]
    pub id: i32,
    #[serde(flatten)]
    pub info: T,
}

pub struct Temps<T> {
    pub list: Vec<Temp<T>>,
}

impl<T> From<Temps<T>> for Dispatch
where
    T: serde::Serialize,
{
    fn from(temps: Temps<T>) -> Self {
        let temp_count = temps.list.len();
        Dispatch::from_success(
            StatusCode::Temps.into(),
            format!("{} Temp(s)", temp_count),
            Some(Body {
                name: "TEMPS",
                list: temps.list,
            }),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Fan {
    #[serde(rename = "FAN")]
    pub idx: i32,
    #[serde(rename = "ID")]
    pub id: i32,
    #[serde(rename = "Speed")]
    pub speed: u32,
    #[serde(rename = "RPM")]
    pub rpm: u32,
}

pub struct Fans {
    pub list: Vec<Fan>,
}

impl From<Fans> for Dispatch {
    fn from(fans: Fans) -> Self {
        let fan_count = fans.list.len();
        Dispatch::from_success(
            StatusCode::Fans.into(),
            format!("{} Fan(s)", fan_count),
            Some(Body {
                name: "FANS",
                list: fans.list,
            }),
        )
    }
}

/// Basic temperature control settings
#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct TunerChainStatus {
    #[serde(rename = "HashchainIndex")]
    pub hashchain_index: u32,
    #[serde(rename = "TuningElapsed")]
    pub tuning_elapsed: Time,
    #[serde(rename = "StageElapsed")]
    pub stage_elapsed: Time,
    #[serde(rename = "Status")]
    pub status: String,
    #[serde(rename = "Iteration")]
    pub iteration: u32,
    #[serde(rename = "PowerLimitWatt")]
    pub power_limit: u32,
    #[serde(rename = "ApproximatePowerConsumptionWatt")]
    pub approx_consumption: u32,
    #[serde(rename = "TunerRunning")]
    pub tuner_running: bool,
}

/// Basic temperature control settings
#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct TunerStatus {
    #[serde(rename = "PowerLimit")]
    pub power_limit: u32,
    #[serde(rename = "ApproximateChainPowerConsumption")]
    pub approx_chain_consumption: u32,
    #[serde(rename = "ApproximateMinerPowerConsumption")]
    pub approx_miner_consumption: u32,
    #[serde(rename = "TunerChainStatus")]
    pub chains: Vec<TunerChainStatus>,
}

impl From<TunerStatus> for Dispatch {
    fn from(tuner_status: TunerStatus) -> Self {
        Dispatch::from_success(
            StatusCode::TunerStatus.into(),
            "Tuner Status".to_string(),
            Some(Body {
                name: "TUNERSTATUS",
                list: vec![tuner_status],
            }),
        )
    }
}
