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

use super::*;

/// Register, Value
pub struct InitReg(pub u8, pub u8);

/// FakeI2cBus is I2C bus backed by an array
#[derive(Clone)]
pub struct FakeI2cBus {
    /// Which I2C address to respond on
    respond_addr: Address,
    /// Memory backing the register space
    /// If `None`, reading/writing this register will cause error.
    data: [Option<u8>; 256],
    /// What to return on addresses other than `respond_addr`
    /// If `None`, reading/writing other addresses will cause error.
    other_addr_response: Option<u8>,
}

impl FakeI2cBus {
    /// Constructs fake bus.
    ///
    /// * `respond_addr` - which I2C address to respond on
    /// * array of `(reg,val)` pairs to populate the array backing the bus
    /// * `other_addr_response` - what to return when empty address is read
    pub fn new(
        respond_addr: Address,
        data: &[InitReg],
        default_value: Option<u8>,
        other_addr_response: Option<u8>,
    ) -> Self {
        let mut bus = Self {
            respond_addr,
            data: [default_value; 256],
            other_addr_response,
        };
        for InitReg(register, value) in data.iter() {
            bus.data[*register as usize] = Some(*value);
        }
        bus
    }
}

#[async_trait]
impl Bus for FakeI2cBus {
    /// Read register from device on I2C bus
    /// if `addr` doesn't match `respond_addr`, return default read byte or error
    /// if `reg` isn't enabled on device, return error
    async fn read(&mut self, addr: Address, reg: u8) -> Result<u8> {
        if addr != self.respond_addr {
            if let Some(val) = self.other_addr_response {
                return Ok(val);
            } else {
                Err(Error::TestInvalidAddress(addr))?
            }
        } else {
            let reg = reg as usize;
            if let Some(val) = self.data[reg] {
                Ok(val)
            } else {
                Err(Error::TestInaccessibleRegister(addr, reg as u8))?
            }
        }
    }

    /// Write register to device on I2C bus
    async fn write(&mut self, addr: Address, reg: u8, val: u8) -> Result<()> {
        // Try read the register first - if it's not accessible, this will create the error
        self.read(addr, reg).await?;

        // Seems that address is accessible, so write to it if it's not default response
        if addr == self.respond_addr {
            self.data[reg as usize] = Some(val);
        }
        Ok(())
    }
}
