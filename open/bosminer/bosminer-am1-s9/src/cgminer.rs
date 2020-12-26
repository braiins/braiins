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

use ii_cgminer_api::command::{DEVDETAILS, FANS, TEMPCTRL, TEMPS};
use ii_cgminer_api::{command, commands, response};

use serde::Serialize;

use std::sync::Arc;

use crate::monitor;
use crate::sensor;

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
#[repr(u32)]
pub enum StatusCode {
    NotReady = 1,
}

impl From<StatusCode> for u32 {
    fn from(code: StatusCode) -> Self {
        code as u32
    }
}

pub enum ErrorCode {
    NotReady,
}

impl From<ErrorCode> for response::Error {
    fn from(code: ErrorCode) -> Self {
        let (code, msg) = match code {
            ErrorCode::NotReady => (StatusCode::NotReady, "Not ready".to_string()),
        };

        Self::from_custom_error(code, msg)
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct DevDetailInfo {
    #[serde(rename = "Voltage")]
    pub voltage: f64,
    #[serde(rename = "Frequency")]
    pub frequency: u32,
    #[serde(rename = "Chips")]
    pub chips: u32,
    #[serde(rename = "Cores")]
    pub cores: u32,
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct TempInfo {
    #[serde(rename = "Board")]
    pub board: f64,
    #[serde(rename = "Chip")]
    pub chip: f64,
}

pub struct Handler {
    model: String,
    managers: Vec<Arc<crate::Manager>>,
    monitor: Arc<monitor::Monitor>,
}

impl Handler {
    pub fn new(
        model: String,
        managers: Vec<Arc<crate::Manager>>,
        monitor: Arc<monitor::Monitor>,
    ) -> Self {
        Self {
            model,
            managers,
            monitor,
        }
    }

    fn get_monitor_status(&self) -> command::Result<monitor::Status> {
        match self.monitor.status_receiver.borrow().clone() {
            Some(status) => Ok(status),
            None => Err(ErrorCode::NotReady.into()),
        }
    }

    async fn handle_dev_details(&self) -> command::Result<response::DevDetails<DevDetailInfo>> {
        let mut list = vec![];
        for manager in self.managers.iter() {
            let inner = manager.inner.lock().await;
            let mut chip_count = 0;
            let mut voltage = 0.0;
            let mut frequency = 0;
            if let Some(hash_chain) = inner.hash_chain.as_ref() {
                chip_count = hash_chain.chip_count;
                voltage = hash_chain.get_voltage().await.as_volts() as f64;
                frequency = hash_chain.get_frequency().await.avg() as u32;
            }
            list.push(response::DevDetail {
                idx: list.len() as i32,
                name: manager.to_string(),
                id: manager.hashboard_idx as i32,
                driver: "".to_string(),
                kernel: "".to_string(),
                model: self.model.clone(),
                device_path: "".to_string(),
                info: DevDetailInfo {
                    voltage,
                    frequency,
                    chips: chip_count as u32,
                    cores: (chip_count * crate::bm1387::NUM_CORES_ON_CHIP) as u32,
                },
            });
        }

        Ok(response::DevDetails { list })
    }

    async fn handle_temp_ctrl(&self) -> command::Result<response::ext::TempCtrl> {
        let config = self.get_monitor_status()?.config;

        let mut mode = response::ext::TempCtrlMode::Disabled;
        let mut target = None;
        let mut hot = None;
        let mut dangerous = None;

        if let Some(temp_config) = config.temp_config {
            mode = response::ext::TempCtrlMode::Manual;
            hot.replace(temp_config.hot_temp);
            dangerous.replace(temp_config.dangerous_temp);
        }
        if let Some(fan_config) = config.fan_config {
            if let monitor::FanControlMode::TargetTemperature(target_temp) = fan_config.mode {
                mode = response::ext::TempCtrlMode::Automatic;
                target.replace(target_temp);
            }
        }

        Ok(response::ext::TempCtrl {
            mode,
            target,
            hot,
            dangerous,
        })
    }

    async fn handle_temps(&self) -> command::Result<response::ext::Temps<TempInfo>> {
        let mut list = vec![];
        for manager in self.managers.iter() {
            let inner = manager.inner.lock().await;
            if let Some(hash_chain) = inner.hash_chain.as_ref() {
                let sensor::Temperature { local, remote } = hash_chain.current_temperature();
                list.push(response::ext::Temp {
                    idx: list.len() as i32,
                    id: manager.hashboard_idx as i32,
                    info: TempInfo {
                        board: Option::from(local).unwrap_or(0.0) as f64,
                        chip: Option::from(remote).unwrap_or(0.0) as f64,
                    },
                });
            }
        }
        Ok(response::ext::Temps { list: list })
    }

    async fn handle_fans(&self) -> command::Result<response::ext::Fans> {
        let status = self.get_monitor_status()?;
        let speed = status.fan_speed.map(|speed| speed.to_pwm()).unwrap_or(0);
        Ok(response::ext::Fans {
            list: status
                .fan_feedback
                .rpm
                .iter()
                .enumerate()
                .map(|(id, rpm)| response::ext::Fan {
                    idx: id as i32,
                    id: id as i32,
                    speed: speed as u32,
                    rpm: *rpm as u32,
                })
                .collect(),
        })
    }
}

pub fn create_custom_commands(
    backend: Arc<crate::Backend>,
    managers: Vec<Arc<crate::Manager>>,
    monitor: Arc<monitor::Monitor>,
) -> command::Map {
    let handler = Arc::new(Handler::new(backend.to_string(), managers, monitor));

    let custom_commands = commands![
        (DEVDETAILS: ParameterLess -> handler.handle_dev_details),
        (TEMPCTRL: ParameterLess -> handler.handle_temp_ctrl),
        (TEMPS: ParameterLess -> handler.handle_temps),
        (FANS: ParameterLess -> handler.handle_fans)
    ];

    custom_commands
}
