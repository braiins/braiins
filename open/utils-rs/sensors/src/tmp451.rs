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

//! Driver implementation of sensor driver for TMP451 and similar sensors

use super::Result;
use super::{Measurement, Sensor, Temperature};
use ii_async_i2c as i2c;

use async_trait::async_trait;
use std::boxed::Box;

const REG_LOCAL_TEMP: u8 = 0x00;
const REG_REMOTE_TEMP: u8 = 0x01;
const REG_STATUS: u8 = 0x02;
const STATUS_OPEN_CIRCUIT: u8 = 0x04;
const REG_CONFIG: u8 = 0x03;
const REG_CONFIG_W: u8 = 0x09;
const CONFIG_RANGE: u8 = 0x04;
const REG_OFFSET: u8 = 0x11;
const REG_REMOTE_FRAC_TEMP: u8 = 0x10;
const REG_LOCAL_FRAC_TEMP: u8 = 0x15;

/// Build a temperature from internal representation
fn make_temp(whole: u8, fract: u8) -> f32 {
    (whole as f32 - 64.0) + (fract as f32 / 256.0)
}

/// Read both local and remote temperatures.
/// Check if external sensor is working properly.
///
/// * `use_fract` - determines if we read and interpret the fractional part of
///   temperature.
///   It makes sense even for sensors that are precise +- 1 degree (because they
///   have internal filtering.
async fn read_temperature(
    i2c_dev: &mut Box<dyn i2c::Device>,
    use_fract: bool,
) -> Result<Temperature> {
    let status = i2c_dev.read(REG_STATUS).await?;
    let local_temp = i2c_dev.read(REG_LOCAL_TEMP).await?;
    let local_frac = if use_fract {
        i2c_dev.read(REG_LOCAL_FRAC_TEMP).await?
    } else {
        0
    };
    let remote_temp = i2c_dev.read(REG_REMOTE_TEMP).await?;
    let remote_frac = if use_fract {
        i2c_dev.read(REG_REMOTE_FRAC_TEMP).await?
    } else {
        0
    };

    let local = Measurement::Ok(make_temp(local_temp, local_frac));
    let remote;
    if (status & STATUS_OPEN_CIRCUIT) != 0 {
        remote = Measurement::OpenCircuit;
    } else if remote_temp == 0 {
        remote = Measurement::ShortCircuit;
    } else {
        remote = Measurement::Ok(make_temp(remote_temp, remote_frac))
    };

    Ok(Temperature { local, remote })
}

/// Read only local temperature
async fn read_temperature_local(i2c_dev: &mut Box<dyn i2c::Device>) -> Result<Temperature> {
    let local_temp = i2c_dev.read(REG_LOCAL_TEMP).await?;

    Ok(Temperature {
        local: Measurement::Ok(make_temp(local_temp, 0)),
        remote: Measurement::NotPresent,
    })
}

async fn generic_init(i2c_dev: &mut Box<dyn i2c::Device>) -> Result<()> {
    i2c_dev
        .write_readback(REG_CONFIG_W, REG_CONFIG, CONFIG_RANGE)
        .await?;
    i2c_dev.write(REG_OFFSET, 0).await?;
    Ok(())
}

/// TMP451 driver (most common type, has remote sensor)
pub struct TMP451 {
    i2c_dev: Box<dyn i2c::Device>,
}

impl TMP451 {
    pub fn new(i2c_dev: Box<dyn i2c::Device>) -> Box<dyn Sensor> {
        Box::new(Self { i2c_dev }) as Box<dyn Sensor>
    }
}

#[async_trait]
impl Sensor for TMP451 {
    async fn init(&mut self) -> Result<()> {
        generic_init(&mut self.i2c_dev).await
    }

    async fn read_temperature(&mut self) -> Result<Temperature> {
        read_temperature(&mut self.i2c_dev, true).await
    }
}

/// ADT7461 driver (almost the same as TMP451)
pub struct ADT7461 {
    i2c_dev: Box<dyn i2c::Device>,
}

impl ADT7461 {
    pub fn new(i2c_dev: Box<dyn i2c::Device>) -> Box<dyn Sensor> {
        Box::new(Self { i2c_dev }) as Box<dyn Sensor>
    }
}

#[async_trait]
impl Sensor for ADT7461 {
    async fn init(&mut self) -> Result<()> {
        generic_init(&mut self.i2c_dev).await
    }

    async fn read_temperature(&mut self) -> Result<Temperature> {
        read_temperature(&mut self.i2c_dev, false).await
    }
}

/// NCT218 driver (only local temperature)
pub struct NCT218 {
    i2c_dev: Box<dyn i2c::Device>,
}

impl NCT218 {
    pub fn new(i2c_dev: Box<dyn i2c::Device>) -> Box<dyn Sensor> {
        Box::new(Self { i2c_dev }) as Box<dyn Sensor>
    }
}

#[async_trait]
impl Sensor for NCT218 {
    async fn init(&mut self) -> Result<()> {
        generic_init(&mut self.i2c_dev).await
    }

    async fn read_temperature(&mut self) -> Result<Temperature> {
        read_temperature_local(&mut self.i2c_dev).await
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use i2c::test_utils::InitReg;
    use ii_async_compat::tokio;

    /// Make sensor T with data being read/written from memory `data`
    fn make_i2c_device(
        data: &[InitReg],
    ) -> i2c::DeviceOnBus<i2c::SharedBus<i2c::test_utils::FakeI2cBus>> {
        let addr = i2c::Address::new(0x16);
        // poison all registers except those we define
        let bus = i2c::test_utils::FakeI2cBus::new(addr, data, None, None);
        let bus = i2c::SharedBus::new(bus);

        i2c::DeviceOnBus::new(bus, addr)
    }

    async fn check_config_ok<T: i2c::Device>(dev: &mut T) {
        assert_eq!(
            dev.read(REG_CONFIG_W).await.unwrap() & CONFIG_RANGE,
            CONFIG_RANGE
        );
        assert_eq!(dev.read(REG_OFFSET).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_sensor_drivers_i2c() {
        let ok_regs = [
            // 23 deg
            InitReg(REG_LOCAL_TEMP, 0x57),
            // 41 deg
            InitReg(REG_REMOTE_TEMP, 0x69),
            InitReg(REG_STATUS, 0x00),
            // .1875 deg
            InitReg(REG_LOCAL_FRAC_TEMP, 0x30),
            // .2500 deg
            InitReg(REG_REMOTE_FRAC_TEMP, 0x40),
            // Config range (this is a little bit of a hack: we pre-set
            // this value so that `write_readback` in driver succeeds.
            InitReg(REG_CONFIG, 0x04),
            // Config range write
            InitReg(REG_CONFIG_W, 0x00),
            // Config offset to 0
            InitReg(REG_OFFSET, 0x7f),
        ];

        // Check "working conditions" on TMP451
        let mut dev = make_i2c_device(&ok_regs);
        let mut sensor = TMP451::new(Box::new(dev.clone()));
        sensor.init().await.unwrap();
        check_config_ok(&mut dev).await;
        assert_eq!(
            sensor.read_temperature().await.unwrap(),
            Temperature {
                local: Measurement::Ok(23.1875),
                remote: Measurement::Ok(41.25),
            }
        );

        // Check "working conditions" on ADT7461
        let mut dev = make_i2c_device(&ok_regs);
        let mut sensor = ADT7461::new(Box::new(dev.clone()));
        sensor.init().await.unwrap();
        check_config_ok(&mut dev).await;
        assert_eq!(
            sensor.read_temperature().await.unwrap(),
            Temperature {
                local: Measurement::Ok(23.0),
                remote: Measurement::Ok(41.0),
            }
        );

        // Check "working conditions" on NCT218
        let mut dev = make_i2c_device(&ok_regs);
        let mut sensor = NCT218::new(Box::new(dev.clone()));
        sensor.init().await.unwrap();
        check_config_ok(&mut dev).await;
        assert_eq!(
            sensor.read_temperature().await.unwrap(),
            Temperature {
                local: Measurement::Ok(23.0),
                remote: Measurement::NotPresent,
            }
        );
    }

    #[tokio::test]
    async fn test_sensor_drivers_i2c_open_circuit() {
        let ok_regs = [
            // 23 deg
            InitReg(REG_LOCAL_TEMP, 0x57),
            // 41 deg
            InitReg(REG_REMOTE_TEMP, 0x69),
            // external sensor is broken-off
            InitReg(REG_STATUS, STATUS_OPEN_CIRCUIT),
            // .1875 deg
            InitReg(REG_LOCAL_FRAC_TEMP, 0x30),
            // .2500 deg
            InitReg(REG_REMOTE_FRAC_TEMP, 0x40),
            // Config range (this is a little bit of a hack: we pre-set
            // this value so that `write_readback` in driver succeeds.
            InitReg(REG_CONFIG, 0x04),
            // Config range write
            InitReg(REG_CONFIG_W, 0x00),
            // Config offset to 0
            InitReg(REG_OFFSET, 0x7f),
        ];

        // Test TMP451
        let mut dev = make_i2c_device(&ok_regs);
        let mut sensor = TMP451::new(Box::new(dev.clone()));
        sensor.init().await.unwrap();
        check_config_ok(&mut dev).await;
        assert_eq!(
            sensor.read_temperature().await.unwrap(),
            Temperature {
                local: Measurement::Ok(23.1875),
                remote: Measurement::OpenCircuit,
            }
        );

        // Test ADT7461
        let mut dev = make_i2c_device(&ok_regs);
        let mut sensor = ADT7461::new(Box::new(dev.clone()));
        sensor.init().await.unwrap();
        check_config_ok(&mut dev).await;
        assert_eq!(
            sensor.read_temperature().await.unwrap(),
            Temperature {
                local: Measurement::Ok(23.0),
                remote: Measurement::OpenCircuit,
            }
        );
    }

    #[tokio::test]
    async fn test_sensor_drivers_i2c_short_circuit() {
        let ok_regs = [
            // 23 deg
            InitReg(REG_LOCAL_TEMP, 0x57),
            // short-circuit
            InitReg(REG_REMOTE_TEMP, 0x00),
            InitReg(REG_STATUS, 0),
            // .1875 deg
            InitReg(REG_LOCAL_FRAC_TEMP, 0x30),
            // .0000 deg
            InitReg(REG_REMOTE_FRAC_TEMP, 0x00),
            // Config range (this is a little bit of a hack: we pre-set
            // this value so that `write_readback` in driver succeeds.
            InitReg(REG_CONFIG, 0x04),
            // Config range write
            InitReg(REG_CONFIG_W, 0x00),
            // Config offset to 0
            InitReg(REG_OFFSET, 0x7f),
        ];

        // Test TMP451
        let mut dev = make_i2c_device(&ok_regs);
        let mut sensor = TMP451::new(Box::new(dev.clone()));
        sensor.init().await.unwrap();
        check_config_ok(&mut dev).await;
        assert_eq!(
            sensor.read_temperature().await.unwrap(),
            Temperature {
                local: Measurement::Ok(23.1875),
                remote: Measurement::ShortCircuit,
            }
        );

        // Test ADT7461
        let mut dev = make_i2c_device(&ok_regs);
        let mut sensor = ADT7461::new(Box::new(dev.clone()));
        sensor.init().await.unwrap();
        check_config_ok(&mut dev).await;
        assert_eq!(
            sensor.read_temperature().await.unwrap(),
            Temperature {
                local: Measurement::Ok(23.0),
                remote: Measurement::ShortCircuit,
            }
        );
    }
}
