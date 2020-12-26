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

//! Driver for I2C bus controllers that can be found in `bm1387` chip.

use async_trait::async_trait;

use crate::bm1387::{self, command::Interface as CommandInterface, ChipAddress};
use ii_async_i2c as i2c;

use crate::error::{self, ErrorKind};
use failure::ResultExt;

use ii_logging::macros::*;

use std::time::Duration;

use ii_async_compat::tokio;
use tokio::time::delay_for;

/// Represents I2C bus that is implemented by sending chip commands
/// to a particular chip on hashchain.
#[derive(Clone)]
pub struct Bus<T: CommandInterface> {
    /// Anything that can issue chip commands
    command_context: T,
    /// Chip address that has I2C bus connected
    chip_address: ChipAddress,
}

/// Implements misc bus commands
impl<T: CommandInterface> Bus<T> {
    /// How many times to try check on I2C bus to become available
    const MAX_I2C_BUSY_WAIT_TRIES: usize = 50;
    /// Timeout in-between busy-wait checks
    const BUSY_WAIT_DELAY: Duration = Duration::from_millis(1);
    /// How many times to try again when I2C command fails (ie. the
    /// address register doesn't match up with what was requested etc.)
    const MAX_I2C_FAIL_TRIES: usize = 5;
    /// Timeout between fails
    const FAIL_TRY_DELAY: Duration = Duration::from_millis(200);

    /// Make new I2C bus.
    /// We init the bus right away to prevent using non-initialized bus.
    pub async fn new_and_init(
        command_context: T,
        chip_address: ChipAddress,
    ) -> error::Result<Self> {
        let mut bus = Self {
            command_context,
            chip_address,
        };
        bus.start().await?;
        Ok(bus)
    }

    /// Wait for I2C controller to become available.
    /// The chip I2C controller sets busy flag when transaction is in progress
    /// and issuing new trasaction may fuck up the internal controller state.
    /// TODO: find out if this can be recovered from
    async fn wait_busy(&mut self) -> error::Result<bm1387::I2cControlReg> {
        for _ in 0..Self::MAX_I2C_BUSY_WAIT_TRIES {
            let reg = self
                .command_context
                .read_one_register::<bm1387::I2cControlReg>(self.chip_address)
                .await?;
            trace!("i2c busy: {:#x?}", reg);
            if !reg.flags.busy {
                // TODO: There was a check for register not being zero - why? (investigate)
                // Should we somehow ensure that the register is not 0 by writing `do_cmd` flag to it?
                // `&& reg.to_reg() != 0`
                return Ok(reg);
            }
            delay_for(Self::BUSY_WAIT_DELAY).await;
        }
        Err(ErrorKind::I2cHashchip(
            "timeout when waiting for I2C response".to_string(),
        ))?
    }

    /// Configure I2C bus on chip.
    /// Instead of writing a completely new configuration to "baudrate"
    /// register, we just alter the bits we need to set (this should avoid
    /// the bug that was present in old versions of cgminer where it did
    /// re-set the `MMEN` flag on chip with temp sensor (thus disabling it
    /// because all work on the chain was with multiple midstates)).
    async fn start(&mut self) -> error::Result<()> {
        let mut misc = self
            .command_context
            .read_one_register::<bm1387::MiscCtrlReg>(self.chip_address)
            .await?;
        misc.set_i2c(Some(bm1387::I2cBusSelect::Bottom));
        self.command_context
            .write_register_readback(self.chip_address, &misc)
            .await?;
        self.wait_busy()
            .await
            .with_context(|_| ErrorKind::I2cHashchip(format!("wating for I2C controller init")))?;

        Ok(())
    }
}

/// I2C bus interface implementation
#[async_trait]
impl<T: CommandInterface> i2c::Bus for Bus<T> {
    async fn write(&mut self, i2c_address: i2c::Address, reg: u8, data: u8) -> i2c::Result<()> {
        let i2c_reg = bm1387::I2cControlReg {
            flags: bm1387::I2cControlFlags {
                do_command: true,
                busy: false,
                // TODO: Implement default
                error: false,
            },
            addr: i2c_address.to_writable_hw_addr(),
            reg,
            data,
        };
        for _ in 0..Self::MAX_I2C_FAIL_TRIES {
            self.wait_busy().await?;
            self.command_context
                .write_register(self.chip_address, &i2c_reg)
                .await?;
            let cmd_reply = self.wait_busy().await?;
            if cmd_reply.flags.error {
                warn!("Hashchip I2C write transaction error! Retrying...");
            } else {
                return Ok(());
            }
            delay_for(Bus::<T>::FAIL_TRY_DELAY).await;
        }
        Err(ErrorKind::I2cHashchip(
            "Hashchip I2C controller keeps getting error".to_string(),
        ))?
    }

    async fn read(&mut self, i2c_address: i2c::Address, reg: u8) -> i2c::Result<u8> {
        let cmd_request = bm1387::I2cControlReg {
            flags: bm1387::I2cControlFlags {
                do_command: true,
                busy: false,
                // TODO: Implement default
                error: false,
            },
            addr: i2c_address.to_readable_hw_addr(),
            reg,
            data: 0,
        };
        for _ in 0..Self::MAX_I2C_FAIL_TRIES {
            self.wait_busy().await?;
            // write I2C READ command
            self.command_context
                .write_register(self.chip_address, &cmd_request)
                .await?;
            // wait for it to be done
            let cmd_reply = self.wait_busy().await?;
            // check for error and that the reply has the same i2c address and  register
            if cmd_reply.flags.error {
                warn!("Hashchip I2C read transaction error! Retrying...");
            } else if cmd_reply.addr == cmd_request.addr && cmd_reply.reg == reg {
                // looks legit
                return Ok(cmd_reply.data);
            }
            delay_for(Bus::<T>::FAIL_TRY_DELAY).await;
        }
        Err(i2c::Error::General(
            "I2C controller keeps reading wrong address/register".to_string(),
        ))
    }
}

/// TODO: add test for frames with error condition
#[cfg(test)]
mod test {
    use super::*;
    use bm1387::{I2cControlReg, MiscCtrlReg, Register};
    use i2c::Bus as I2cBus;
    use std::sync::Arc;

    use futures::lock::Mutex;
    use ii_async_compat::futures;

    /// This trait simulates "chip" on hashchain whose registers can be read or written.
    trait RegisterInterface: Send + Sync {
        /// Read chip register and return its value of `None` if this
        /// register doesn't exists (could be propagated as error to the
        /// caller).
        fn read_reg(&mut self, reg: u8) -> Option<u32>;
        /// Write register, return `Some(())` if this register exists
        /// or `None` when it doesn't.
        fn write_reg(&mut self, reg: u8, value: u32) -> Option<()>;
        /// Each chip has associated an address (required so that we can address
        /// it on hashchain).
        fn get_address(&self) -> ChipAddress;
    }

    /// Locking wrapper
    struct SharedRegisterInterface<R> {
        inner: Arc<Mutex<R>>,
    }

    impl<R: RegisterInterface> SharedRegisterInterface<R> {
        fn new(t: R) -> Self {
            Self {
                inner: Arc::new(Mutex::new(t)),
            }
        }
    }

    impl<R> std::clone::Clone for SharedRegisterInterface<R> {
        fn clone(&self) -> Self {
            Self {
                inner: self.inner.clone(),
            }
        }
    }

    /// If anything simulates chip register, then implement command
    /// interface atop of it, so that we can use it as backend for our
    /// `hashchip` I2C bus implementation.
    #[async_trait]
    impl<R: RegisterInterface> CommandInterface for SharedRegisterInterface<R> {
        /// Read register
        async fn read_register<T: Register>(
            &self,
            chip_address: ChipAddress,
        ) -> error::Result<Vec<T>> {
            let mut inner = self.inner.lock().await;
            // there shouldn't be any other communication on the bus
            assert!(chip_address == inner.get_address());
            // check if register exists
            match inner.read_reg(T::REG_NUM) {
                None => panic!("register {:#x} shouldn't be read", T::REG_NUM),
                Some(value) => Ok(vec![T::from_reg(value)]),
            }
        }

        /// Write register
        async fn write_register<'a, T: Register>(
            &'a self,
            chip_address: ChipAddress,
            value: &'a T,
        ) -> error::Result<()> {
            let mut inner = self.inner.lock().await;
            assert!(chip_address == inner.get_address());
            // check if register exists
            match inner.write_reg(T::REG_NUM, value.to_reg()) {
                None => panic!("register {:#x} shoudln't be written", T::REG_NUM),
                Some(_) => Ok(()),
            }
        }
    }

    /// Test that i2c bus on chip has been correctly initialized
    struct CheckInit {
        /// Our address
        sensor_address: ChipAddress,
        /// Value of Misc register (holds baudrate and i2c mux settings)
        misc_reg: u32,
    }

    impl CheckInit {
        fn new(sensor_address: ChipAddress) -> Self {
            Self {
                sensor_address,
                // initial misc register settings
                misc_reg: 0x00_20_01_80,
            }
        }
        /// function to verify that all registers have been written successfully
        fn verify_regs_ok(&self) {
            // check only for i2c mux settings
            // This register current value depends on the initial value set in `new`.
            assert_eq!(self.misc_reg, 0x40_20_41_e0);
        }
    }

    /// Implement only registers required during init
    impl RegisterInterface for CheckInit {
        fn read_reg(&mut self, reg: u8) -> Option<u32> {
            match reg {
                // we are always ready (non-busy)
                I2cControlReg::REG_NUM => Some(0x01_00_00_00),
                // Misc register is stored
                MiscCtrlReg::REG_NUM => Some(self.misc_reg),
                _ => None,
            }
        }

        fn write_reg(&mut self, reg: u8, value: u32) -> Option<()> {
            match reg {
                // allow writes only to misc register (no transactions should
                // be issued during initialization anyway)
                MiscCtrlReg::REG_NUM => self.misc_reg = value,
                _ => return None,
            }
            Some(())
        }

        fn get_address(&self) -> ChipAddress {
            self.sensor_address
        }
    }

    #[tokio::test]
    async fn test_hashchip_i2c_init() {
        // Construct chip with I2C bus attached
        let sensor_address = ChipAddress::One(0x14);
        // Chip registers are backed with our `CheckInit` "emulator"
        let regs = CheckInit::new(sensor_address);
        let shared_regs = SharedRegisterInterface::new(regs);
        // Construct bus with the chip as backend and initialize it
        Bus::new_and_init(shared_regs.clone(), sensor_address)
            .await
            .expect("initialization failed");
        // Check the result is OK
        shared_regs.inner.lock().await.verify_regs_ok();
    }

    /// More convoluted test: check that:
    ///
    /// 1. read and write return expected values
    /// 2. the bus implementation wait for chip to be non-busy
    struct CheckReadWrite {
        sensor_address: ChipAddress,
        /// value of `MiscCtrl` register (with i2c configuration)
        misc_reg: u32,
        /// value of `I2cControl` register
        i2c_reg: u32,
        /// test value was read OK
        read_ok: bool,
        /// test value was written OK
        write_ok: bool,
        /// if non-zero, then on the next read/write calls simulate that
        /// many busy conditions
        busy_ticks: usize,
    }

    impl CheckReadWrite {
        fn new(sensor_address: ChipAddress) -> Self {
            Self {
                sensor_address,
                // initial register values
                misc_reg: 0x00_20_01_80,
                i2c_reg: 0,
                // test have not yet passed
                read_ok: false,
                write_ok: false,
                // we are initially busy
                busy_ticks: 25,
            }
        }

        fn verify_regs_ok(&self) {
            assert!(self.read_ok);
            assert!(self.write_ok);
            assert_eq!(self.misc_reg, 0x40_20_41_e0);
            // we shouldn't be busy after test finished (or someone
            // forgot to wait)
            assert_eq!(self.busy_ticks, 0);
        }
    }

    /// What to read/write during test
    const TEST_READ_ADDR: u8 = 0x34;
    const TEST_READ_REG: u8 = 0x57;
    const TEST_READ_VAL: u8 = 0x31;
    const TEST_WRITE_ADDR: u8 = 0x18;
    const TEST_WRITE_REG: u8 = 0x11;
    const TEST_WRITE_VAL: u8 = 0xfe;

    impl RegisterInterface for CheckReadWrite {
        fn read_reg(&mut self, reg: u8) -> Option<u32> {
            match reg {
                I2cControlReg::REG_NUM => {
                    trace!("busy_ticks={} reg={:#x}", self.busy_ticks, self.i2c_reg);
                    if self.busy_ticks > 0 {
                        // simulate requested busy ticks
                        self.busy_ticks -= 1;
                        // return nothing meaningful
                        Some(0x80_00_00_00)
                    } else {
                        // return the result of transaction
                        Some(self.i2c_reg)
                    }
                }
                MiscCtrlReg::REG_NUM => Some(self.misc_reg),
                // invalid register
                _ => None,
            }
        }

        fn write_reg(&mut self, reg: u8, value: u32) -> Option<()> {
            match reg {
                MiscCtrlReg::REG_NUM => self.misc_reg = value,
                I2cControlReg::REG_NUM => {
                    // check that we were initialized
                    assert_eq!(self.misc_reg, 0x40_20_41_e0);
                    // check that we were not written when busy
                    assert_eq!(self.busy_ticks, 0);
                    // check that `do_command` flag has been set
                    assert!((value & 0x01_00_00_00) != 0);

                    // do command
                    if (value & 0x00_01_00_00) != 0 {
                        // i2c write
                        let expected = 0x01_01_00_00
                            | ((TEST_WRITE_ADDR as u32) << 16)
                            | ((TEST_WRITE_REG as u32) << 8)
                            | (TEST_WRITE_VAL as u32);
                        // expect particular transaction
                        assert_eq!(value, expected);
                        self.i2c_reg = expected;
                        // this part of test passed
                        self.write_ok = true;
                    } else {
                        // i2c read
                        let expected = 0x01_00_00_00
                            | ((TEST_READ_ADDR as u32) << 16)
                            | ((TEST_READ_REG as u32) << 8);
                        // expect particular transaction
                        assert_eq!(value, expected);
                        self.i2c_reg = expected | TEST_READ_VAL as u32;
                        // this part of test passed
                        self.read_ok = true;
                    }
                    // simulate business on the next access
                    self.busy_ticks = 10;
                }
                // invalid register
                _ => return None,
            }
            Some(())
        }

        fn get_address(&self) -> ChipAddress {
            self.sensor_address
        }
    }

    #[tokio::test]
    async fn test_hashchip_i2c_read_write() {
        // construct fake chip
        let sensor_address = ChipAddress::One(0x14);
        let regs = CheckReadWrite::new(sensor_address);
        let shared_regs = SharedRegisterInterface::new(regs);
        // create i2c bus backed with that chip
        let mut bus = Bus::new_and_init(shared_regs.clone(), sensor_address)
            .await
            .expect("initialization failed");
        // issue read transaction
        assert_eq!(
            bus.read(i2c::Address::new(TEST_READ_ADDR), TEST_READ_REG)
                .await
                .expect("i2c read failed"),
            TEST_READ_VAL
        );
        // issue write transaction
        bus.write(
            i2c::Address::new(TEST_WRITE_ADDR),
            TEST_WRITE_REG,
            TEST_WRITE_VAL,
        )
        .await
        .expect("i2c writefailed");
        // verify everything went fine
        shared_regs.inner.lock().await.verify_regs_ok();
    }
}
