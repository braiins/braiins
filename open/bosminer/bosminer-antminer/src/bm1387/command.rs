// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU Common Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Common Public License for more details.
//
// You should have received a copy of the GNU Common Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

//! This module implements API (`Interface`) for sending and receiving commands to
//! chips.
//!
//! There's also implementation (`InnerContext`) of that interface that can send and receive
//! commands via `command_io` FPGA register (+ shared version).

use ii_logging::macros::*;

use async_trait::async_trait;

use crate::bm1387::{self, ChipAddress};
use crate::io;
use std::time::Duration;

use packed_struct::{PackedStruct, PackedStructSlice};

use futures::lock::Mutex;
use ii_async_compat::futures;
use std::sync::Arc;

use crate::error::{self, ErrorKind};
use failure::ResultExt;

/// Interface definition for command-stack API - reading and writing of registers
///
/// Some functions have blanket implementation for ease of use.
#[async_trait]
pub trait Interface: Send + Sync {
    /// Read register(s) and collect replies
    ///
    /// * `chip_address` can address one or more chips
    /// * register number is pulled from register type `T`
    async fn read_register<T: bm1387::Register>(
        &self,
        chip_address: ChipAddress,
    ) -> error::Result<Vec<T>>;

    /// Write register(s)
    ///
    /// * `chip_address` can address one or more chips
    async fn write_register<'a, T: bm1387::Register>(
        &'a self,
        chip_address: ChipAddress,
        value: &'a T,
    ) -> error::Result<()>;

    /// Read exactly one register and return reply
    ///
    /// * `chip_address` can be only unicast
    async fn read_one_register<T: bm1387::Register>(
        &self,
        chip_address: ChipAddress,
    ) -> error::Result<T> {
        assert!(!chip_address.is_broadcast());
        let mut responses = self.read_register::<T>(chip_address).await?;
        return Ok(responses.remove(0));
    }

    /// Write register(s) and read it/them back to verify they were written correctly
    /// Same as `write_register`, but followed by `read_register` on the same register.
    async fn write_register_readback<'a, T: bm1387::Register>(
        &'a self,
        chip_address: ChipAddress,
        value: &'a T,
    ) -> error::Result<()> {
        // write register
        self.write_register(chip_address, value).await?;

        // do readback
        let responses = self.read_register::<T>(chip_address).await?;
        for (chip_address, read_back_value) in responses.iter().enumerate() {
            if *read_back_value != *value {
                Err(ErrorKind::Hashchip(format!(
                    "chip {} returned wrong value of register {:#x}: {:#x?} instead of {:#x?}",
                    chip_address,
                    T::REG_NUM,
                    *read_back_value,
                    value
                )))?
            }
        }
        Ok(())
    }
}

/// `InnerContext` holds FPGA registers with command FIFO and implements on top
/// of them functions to issue commands to chip registers (via `send_raw_command`)
/// or to read/write chip registers (via `Interface` interface).
///
/// No locking for sharing is provided.
pub struct InnerContext {
    /// s9-io FPGA registers
    command_io: io::CommandRxTx,
    /// Number of chips on chain - used to verify all replies have been received.
    /// If `chip_count` is `None`, number of chips haven't been determined yet so
    /// skip the check.
    chip_count: Option<usize>,
}

/// Interface to access chip registers via series of commands
impl InnerContext {
    /// Timeout for waiting for command
    const COMMAND_READ_TIMEOUT: Duration = Duration::from_millis(100);

    /// How long to wait for command RX queue flush
    const COMMAND_FLUSH_TIMEOUT: Duration = Duration::from_micros(5);

    /// Read register(s)
    ///
    /// Throw an error if unexpected number of replies have been received.
    /// (expected number is one reply per chip)
    async fn read_register<T: bm1387::Register>(
        &mut self,
        chip_address: ChipAddress,
    ) -> error::Result<Vec<T>> {
        let cmd = bm1387::GetStatusCmd::new(chip_address, T::REG_NUM);
        // send command, do not wait for it to be sent out
        self.command_io
            .send_command(cmd.pack().to_vec(), false)
            .await;

        // wait for all responses and collect them
        let mut responses = Vec::new();
        loop {
            match self
                .command_io
                .recv_response(Self::COMMAND_READ_TIMEOUT)
                .await?
            {
                Some(one_response) => {
                    let one_response = bm1387::CmdResponse::unpack_from_slice(&one_response)
                        .context(format!("response unpacking failed"))?;
                    responses.push(one_response.value);
                    // exit early if we expect just one response
                    if chip_address != ChipAddress::All {
                        break;
                    }
                }
                None => break,
            }
        }

        // figure out how many responses are we expecting, and thrown an error
        // if less were received
        if chip_address == ChipAddress::All {
            if let Some(chip_count) = self.chip_count {
                // for broadcast we expect chip_count responses
                if chip_count != responses.len() {
                    Err(ErrorKind::Hashchip(format!(
                        "Number of responses {} of GetStatusCmd(reg={:#x}) doesn't match chip count {}",
                        responses.len(),
                        T::REG_NUM,
                        chip_count
                    )))?;
                }
            }
        } else {
            if responses.len() != 1 {
                Err(ErrorKind::Hashchip(format!(
                    "No response for GetStatusCmd(reg={:#x}) from chip {:?}",
                    T::REG_NUM,
                    chip_address
                )))?;
            }
        }

        // convert to registers
        Ok(responses
            .into_iter()
            .map(|x| T::from_reg(x))
            .collect::<Vec<T>>())
    }

    async fn flush_command_rx(&mut self) -> error::Result<()> {
        while let Some(response) = self
            .command_io
            .recv_response(Self::COMMAND_FLUSH_TIMEOUT)
            .await?
        {
            warn!("extra garbage command response: {:#x?}", response);
        }
        Ok(())
    }

    /// Write register(s)
    async fn write_register<'a, T: bm1387::Register>(
        &'a mut self,
        chip_address: ChipAddress,
        value: &'a T,
    ) -> error::Result<()> {
        let cmd = bm1387::SetConfigCmd::new(chip_address, T::REG_NUM, value.to_reg());
        // wait for command to be sent out
        self.command_io
            .send_command(cmd.pack().to_vec(), true)
            .await;
        // This is workaround for chips sending garbage when they transmit nonce while someone
        // changes their PLL: sometimes the garbage can have correct CRC and command bit set.
        // Then we get unsolicited message in our command-rx queue and the next read register
        // command will complain about too many replies.
        // We flush the command queue here.
        self.flush_command_rx().await?;
        Ok(())
    }

    /// Send raw command without any explicit serialization.
    /// If `wait` is true, wait for the command to be issued.
    async fn send_raw_command(&mut self, cmd: Vec<u8>, wait: bool) {
        self.command_io.send_command(cmd, wait).await;
    }

    /// Set number of chips on chain (and implicitly enable check for
    /// number of replies on broadcast messages)
    fn set_chip_count(&mut self, chip_count: usize) {
        self.chip_count = Some(chip_count);
    }

    pub fn new(command_io: io::CommandRxTx) -> Self {
        Self {
            command_io,
            chip_count: None,
        }
    }
}

/// Locking wrapper on InnerContext. Implements Interface.
#[derive(Clone)]
pub struct Context {
    inner: Arc<Mutex<InnerContext>>,
}

#[async_trait]
impl Interface for Context {
    async fn read_register<T: bm1387::Register>(
        &self,
        chip_address: ChipAddress,
    ) -> error::Result<Vec<T>> {
        let mut inner = self.inner.lock().await;
        inner.read_register::<T>(chip_address).await
    }

    async fn write_register<'a, T: bm1387::Register>(
        &'a self,
        chip_address: ChipAddress,
        value: &'a T,
    ) -> error::Result<()> {
        let mut inner = self.inner.lock().await;
        inner.write_register(chip_address, value).await
    }
}

impl Context {
    pub async fn send_raw_command(&self, cmd: Vec<u8>, wait: bool) {
        let mut inner = self.inner.lock().await;
        inner.send_raw_command(cmd, wait).await
    }

    pub async fn set_chip_count(&self, chip_count: usize) {
        let mut inner = self.inner.lock().await;
        inner.set_chip_count(chip_count);
    }

    pub fn new(command_io: io::CommandRxTx) -> Self {
        Self {
            inner: Arc::new(Mutex::new(InnerContext::new(command_io))),
        }
    }
}
