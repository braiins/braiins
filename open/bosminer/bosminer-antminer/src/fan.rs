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

//! This module is responsible for reading fan feedback and setting fan PWM in FPGA controller.

pub mod pid;

use std::fmt;

use crate::error::{self, ErrorKind};
use failure::ResultExt;

use uio_async;

/// Structure representing PWM of fan
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Speed(usize);

impl Speed {
    pub const FULL_SPEED: Self = Self(100);
    pub const STOPPED: Self = Self(0);

    pub fn new(speed: usize) -> Self {
        assert!(speed <= 100);

        Speed(speed)
    }

    pub fn to_pwm(&self) -> usize {
        self.0
    }
}

impl fmt::Display for Speed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.0)
    }
}

/// Speed of fans read from feedback pins
#[derive(Debug, Clone)]
pub struct Feedback {
    pub rpm: Vec<usize>,
}

impl Feedback {
    pub fn num_fans_running(&self) -> usize {
        self.rpm.iter().filter(|rpm| **rpm > 0).count()
    }
}

impl fmt::Display for Feedback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.rpm
                .iter()
                .map(|&rpm| if rpm >= 1000 {
                    format!("{:.1}K", rpm as f32 / 1000.0)
                } else {
                    format!("{}", rpm)
                })
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

/// Memory-mapped fan controller
pub struct Control {
    regs: uio_async::UioTypedMapping<ii_fpga_io_am1_s9::fan_ctrl::RegisterBlock>,
}

impl Control {
    pub fn new() -> error::Result<Self> {
        let name = "fan-control".to_string();
        let uio = uio_async::UioDevice::open_by_name(&name).with_context(|_| {
            ErrorKind::UioDevice(name.clone(), "cannot find uio device".to_string())
        })?;
        let map = uio.map_mapping(0).with_context(|_| {
            ErrorKind::UioDevice(name.clone(), "cannot map uio device".to_string())
        })?;

        Ok(Self {
            regs: map.into_typed(),
        })
    }

    /// Read feedback registers and convert them to RPM
    pub fn read_feedback(&self) -> Feedback {
        Feedback {
            rpm: self
                .regs
                .fan_rps
                .iter()
                .map(|rps| rps.read().bits() as usize * 60)
                .collect::<Vec<usize>>(),
        }
    }

    /// Set PWM for fans in percent (0 means fans stopped, 100 means fans on full)
    pub fn set_speed(&self, speed: Speed) {
        // Only lower 8 bits of FAN_PWM register are considered, so writing 256 would stop fans,
        // hence the assert.
        assert!(speed.0 <= 100);
        self.regs
            .fan_pwm
            .write(|w| unsafe { w.bits(speed.0 as u8) })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_fan_speed() {
        assert_eq!(Speed::STOPPED.0, 0);
        assert_eq!(Speed::FULL_SPEED.0, 100);
        assert_eq!(Speed::new(70).0, 70);
    }

    #[test]
    #[should_panic]
    fn test_fan_speed_fail() {
        Speed::new(101);
    }

    #[test]
    fn test_feedback_fan_count() {
        assert_eq!(
            Feedback {
                rpm: vec![50, 0, 11, 0, 0]
            }
            .num_fans_running(),
            2
        );
        assert_eq!(
            Feedback {
                rpm: vec![0, 0, 0, 0, 0]
            }
            .num_fans_running(),
            0
        );
        assert_eq!(Feedback { rpm: Vec::new() }.num_fans_running(), 0);
    }
}
