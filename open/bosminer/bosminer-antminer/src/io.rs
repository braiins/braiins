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

//! This module provides API to access memory-mapped FPGA registers and associated interrupts.
//!
//! It is split into two layers:
//!   * `Io` layer, which provides interface to FPGA registers and implements
//!     API to wait for events (via interrupts)
//!   * `Control` layer knows about chip configuration (number of midstates)
//!     and implements few higher-level functions to read/write work

mod ext_work_id;
mod uio;

use crate::error::{self, ErrorKind};
use ext_work_id::ExtWorkId;

use bosminer::work;
use std::convert::TryInto;
use std::fmt;

use chrono::prelude::DateTime;
use chrono::Utc;
use std::time::{Duration, UNIX_EPOCH};

use ii_async_compat::prelude::*;
use tokio::time::delay_for;

use crate::MidstateCount;
use ii_fpga_io_am1_s9::{
    self,
    common::{ctrl_reg::MIDSTATE_CNT_A, version::MINER_TYPE_A},
    generic::Variant,
};

use ii_logging::macros::*;

/// We fail the initialization unless we find this s9-io of this version
const EXPECTED_S9IO_VERSION: Version = Version {
    miner_type: MinerType::Known(MINER_TYPE_A::ANTMINER),
    model: 9,
    major: 1,
    minor: 0,
    patch: 0,
};

/// Base clock speed of the IP core running in the FPGA
pub const F_CLK_SPEED_HZ: usize = 50_000_000;
/// Divisor of the base clock. The resulting clock is connected to UART
pub const F_CLK_BASE_BAUD_DIV: usize = 8;

/// Helper method to convert seconds to FPGA ticks suitable to be written
/// to `WORK_TIME` FPGA register.
///
/// Returns number of ticks.
pub fn secs_to_fpga_ticks(secs: f64) -> u32 {
    (secs * F_CLK_SPEED_HZ as f64) as u32
}

/// Util structure to help us work with enums
#[derive(Debug, Clone, PartialEq)]
enum MinerType {
    Known(MINER_TYPE_A),
    Unknown(usize),
}

/// Structure representing the build time from register `BUILD_ID`
struct BuildId(u32);

impl BuildId {
    fn seems_legit(&self) -> bool {
        // bitstream created after 2019 and before 2038
        self.0 > 1546300800 && self.0 < 0x8000_0000
    }
}

impl fmt::Display for BuildId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Creates a new SystemTime from the specified number of whole seconds
        let d = UNIX_EPOCH + Duration::from_secs(self.0.into());
        // Create DateTime from SystemTime
        let datetime = DateTime::<Utc>::from(d);
        // Formats the combined date and time with the specified format string.
        write!(f, "{}", datetime.format("%Y-%m-%d %H:%M:%S %Z"))
    }
}

/// Structure representing `VERSION` register
#[derive(Debug, Clone, PartialEq)]
struct Version {
    miner_type: MinerType,
    model: usize,
    major: usize,
    minor: usize,
    patch: usize,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let model;
        match self.miner_type {
            MinerType::Known(MINER_TYPE_A::ANTMINER) => model = format!("Antminer S{}", self.model),
            MinerType::Unknown(n) => model = format!("Unknown[{}, {}]", n, self.model),
        }

        write!(
            f,
            "{}.{}.{} for {}",
            self.major, self.minor, self.patch, model
        )
    }
}

#[derive(Clone, Debug)]
pub struct Solution {
    /// Actual nonce
    pub nonce: u32,
    /// Index of a midstate that corresponds to the found nonce
    pub midstate_idx: usize,
    /// Index of a solution (if multiple were found)
    pub solution_idx: usize,
    /// Hardware specific solution identifier
    pub hardware_id: u32,
}

struct WorkRxFifo {
    regs: uio_async::UioTypedMapping<ii_fpga_io_am1_s9::workrx::RegisterBlock>,
    uio: uio_async::UioDevice,
}

impl WorkRxFifo {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.regs.work_rx_stat_reg.read().rx_empty().bit()
    }

    /// Try to read from work rx fifo.
    /// Performs blocking read with timeout. Uses IRQ.
    #[allow(dead_code)]
    #[inline]
    pub fn read(&mut self, timeout: Option<Duration>) -> error::Result<Option<u32>> {
        let cond = || !self.is_empty();
        let got_irq = self.uio.irq_wait_cond(cond, timeout)?;
        Ok(got_irq.and_then(|_| Some(self.regs.work_rx_fifo.read().bits())))
    }

    /// Try to read from work rx fifo.
    /// Async variant. Uses IRQ.
    pub async fn async_read(&mut self) -> error::Result<u32> {
        let cond = || !self.is_empty();
        self.uio.async_irq_wait_cond(cond).await?;
        Ok(self.regs.work_rx_fifo.read().bits())
    }

    pub fn init(&mut self) -> error::Result<()> {
        // reset input FIFO
        self.regs
            .work_rx_ctrl_reg
            .modify(|_, w| w.rst_rx_fifo().set_bit());
        // enable IRQ_WORK_RX interrupt
        self.regs
            .work_rx_ctrl_reg
            .modify(|_, w| w.irq_en().set_bit());
        Ok(())
    }

    pub fn new(hashboard_idx: usize) -> error::Result<Self> {
        let uio = uio::Device::open(hashboard_idx, uio::Type::WorkRx)?;
        Ok(Self {
            regs: uio.map()?,
            uio: uio.uio,
        })
    }
}

struct WorkTxFifo {
    regs: uio_async::UioTypedMapping<ii_fpga_io_am1_s9::worktx::RegisterBlock>,
    uio: uio_async::UioDevice,
}

impl WorkTxFifo {
    /// FIFO size (in u32 words)
    const FIFO_SIZE: u32 = 2048;

    /// Bigget work size (in u32 words)
    const BIGGEST_WORK: u32 = 200;

    /// Threshold for number of entries in FIFO queue under which we recon we could
    /// fit one more work.
    const FIFO_THRESHOLD: u32 = Self::FIFO_SIZE - Self::BIGGEST_WORK;

    #[inline]
    pub fn is_full(&self) -> bool {
        self.regs.work_tx_stat_reg.read().tx_full().bit()
    }

    #[inline]
    pub fn has_space_for_one_job(&self) -> bool {
        self.regs.work_tx_stat_reg.read().irq_pend().bit()
    }

    /// Return the value of last work ID send to ASICs
    #[inline]
    #[allow(dead_code)]
    pub fn get_last_work_id(&mut self) -> u32 {
        self.regs.work_tx_last_id.read().bits()
    }

    /// Try to write work item to work TX FIFO.
    /// Performs blocking write without timeout. Uses IRQ.
    /// The idea is that you don't call this function until you are sure you
    /// can fit in all the entries you want.
    #[inline]
    pub fn write(&mut self, item: u32) -> error::Result<()> {
        let cond = || !self.is_full();
        self.uio.irq_wait_cond(cond, None)?;
        self.regs.work_tx_fifo.write(|w| unsafe { w.bits(item) });
        Ok(())
    }

    /// Wait for output FIFO to make room for one work
    pub async fn async_wait_for_room(&self) -> error::Result<()> {
        let cond = || self.has_space_for_one_job();
        self.uio.async_irq_wait_cond(cond).await?;
        Ok(())
    }

    pub fn init(&mut self) -> error::Result<()> {
        // Set threshold for work TX so that there's space for
        // at least one job.
        self.regs
            .work_tx_irq_thr
            .write(|w| unsafe { w.bits(Self::FIFO_THRESHOLD) });
        // reset output FIFO
        self.regs
            .work_tx_ctrl_reg
            .modify(|_, w| w.rst_tx_fifo().set_bit());
        // enable IRQ_WORK_TX interrupt
        self.regs
            .work_tx_ctrl_reg
            .modify(|_, w| w.irq_en().set_bit());
        Ok(())
    }

    pub fn new(hashboard_idx: usize) -> error::Result<Self> {
        let uio = uio::Device::open(hashboard_idx, uio::Type::WorkTx)?;
        Ok(Self {
            regs: uio.map()?,
            uio: uio.uio,
        })
    }
}

/// This object drives both FIFOs, because we handle command responses
/// in a task synchronously.
///
/// TODO: Split this FIFO into two FIFOs.
pub struct CommandRxTxFifos {
    regs: uio_async::UioTypedMapping<ii_fpga_io_am1_s9::command::RegisterBlock>,
    uio: uio_async::UioDevice,
}

impl CommandRxTxFifos {
    #[inline]
    pub fn get_stat_reg(&self) -> u32 {
        self.regs.cmd_stat_reg.read().bits()
    }

    #[inline]
    pub fn is_rx_empty(&self) -> bool {
        self.regs.cmd_stat_reg.read().rx_empty().bit()
    }

    #[inline]
    pub fn is_tx_empty(&self) -> bool {
        self.regs.cmd_stat_reg.read().tx_empty().bit()
    }

    #[inline]
    pub fn is_tx_full(&self) -> bool {
        self.regs.cmd_stat_reg.read().tx_full().bit()
    }

    /// Wait for command FIFO to become empty
    /// Uses timed polling
    pub async fn wait_tx_empty(&self) {
        while !self.is_tx_empty() {
            delay_for(Duration::from_millis(1)).await;
        }
    }

    /// Write command to cmd tx fifo.
    /// Uses timed polling
    pub async fn write(&self, item: u32) {
        // wait for space in queue
        while self.is_tx_full() {
            delay_for(Duration::from_millis(1)).await;
        }
        // write command word
        self.regs.cmd_tx_fifo.write(|w| unsafe { w.bits(item) });
    }

    /// Read command from cmd rx fifo
    /// Async variant. Uses IRQ.
    pub async fn read(&mut self) -> error::Result<u32> {
        let cond = || !self.is_rx_empty();
        self.uio.async_irq_wait_cond(cond).await?;
        Ok(self.regs.cmd_rx_fifo.read().bits())
    }

    /// Read command from cmd rx fifo with timeout
    /// Async variant. Uses IRQ.
    /// Returns:
    ///     * `Ok(None)` on timeout
    ///     * `Ok(Some(_))` if something was received
    ///     * `Err(_)` if error occured
    pub async fn read_with_timeout(&mut self, timeout: Duration) -> error::Result<Option<u32>> {
        match self.read().timeout(timeout).await {
            Ok(Ok(word)) => Ok(Some(word)), // Read complete on time
            Ok(Err(err)) => Err(err),       // Read I/O error
            Err(_) => {
                // Read timeout
                if !self.is_rx_empty() {
                    // XXX workaround when cpu is 100% full
                    Ok(Some(self.read().await?))
                } else {
                    Ok(None)
                }
            }
        }
    }

    pub fn init(&mut self) -> error::Result<()> {
        // reset input FIFO
        self.regs
            .cmd_ctrl_reg
            .modify(|_, w| w.rst_rx_fifo().set_bit().rst_tx_fifo().set_bit());
        // enable IRQ_CMD_RX interrupt
        self.regs.cmd_ctrl_reg.modify(|_, w| w.irq_en().set_bit());
        Ok(())
    }

    pub fn new(hashboard_idx: usize) -> error::Result<Self> {
        let uio = uio::Device::open(hashboard_idx, uio::Type::Command)?;
        Ok(Self {
            regs: uio.map()?,
            uio: uio.uio,
        })
    }
}

/// This structure represents mining solution response as read from
/// `WORK_RX_FIFO` in FPGA.
#[derive(Debug, Clone)]
struct WorkRxResponse {
    pub nonce: u32,
    pub work_id: usize,
    pub midstate_idx: usize,
    pub solution_idx: usize,
}

impl WorkRxResponse {
    /// Parse from FPGA response
    /// The format is dependent on current `MidstateCount` settings
    pub fn from_hw(midstate_count: MidstateCount, word1: u32, word2: u32) -> Self {
        // NOTE: there's a CRC field in word2 that we ignore, because it's checked by FPGA core
        let solution_idx = word2 & 0xff;
        let ext_work_id = (word2 >> 8) & 0xffff;
        let ext_work_id = ExtWorkId::from_hw(midstate_count, ext_work_id);
        Self {
            nonce: word1,
            solution_idx: solution_idx as usize,
            work_id: ext_work_id.work_id,
            midstate_idx: ext_work_id.midstate_idx,
        }
    }
}

pub struct WorkRx {
    fifo: WorkRxFifo,
    midstate_count: MidstateCount,
}

impl WorkRx {
    pub async fn recv_solution(mut self) -> Result<(Self, Solution), failure::Error> {
        let word1 = self.fifo.async_read().await?;
        let word2 = self.fifo.async_read().await?;
        let resp = WorkRxResponse::from_hw(self.midstate_count, word1, word2);

        let solution = Solution {
            nonce: resp.nonce,
            midstate_idx: resp.midstate_idx,
            solution_idx: resp.solution_idx,
            hardware_id: resp.work_id as u32,
        };

        Ok((self, solution))
    }

    fn init(&mut self) -> error::Result<()> {
        self.fifo.init()
    }

    fn new(hashboard_idx: usize, midstate_count: MidstateCount) -> error::Result<Self> {
        Ok(Self {
            fifo: WorkRxFifo::new(hashboard_idx)?,
            midstate_count,
        })
    }
}

pub struct WorkTx {
    fifo: WorkTxFifo,
    midstate_count: MidstateCount,
}

impl WorkTx {
    pub async fn wait_for_room(&self) -> error::Result<()> {
        self.fifo.async_wait_for_room().await
    }

    pub fn assert_midstate_count(&self, expected_midstate_count: usize) {
        assert_eq!(
            expected_midstate_count,
            self.midstate_count.to_count(),
            "Outgoing work has {} midstates, but miner is configured for {} midstates!",
            expected_midstate_count,
            self.midstate_count.to_count(),
        );
    }

    pub fn send_work(
        &mut self,
        work: &work::Assignment,
        work_id: usize,
    ) -> Result<(), failure::Error> {
        self.assert_midstate_count(work.midstates.len());
        let ext_work_id = ExtWorkId::new(work_id, 0);

        self.fifo
            .write(ext_work_id.to_hw(self.midstate_count).to_le())?;
        self.fifo.write(work.bits().to_le())?;
        self.fifo.write(work.ntime.to_le())?;
        self.fifo.write(work.merkle_root_tail().to_le())?;

        for mid in work.midstates.iter() {
            for midstate_word in mid.state.words::<u32>().rev() {
                self.fifo.write(midstate_word.to_be())?;
            }
        }
        Ok(())
    }

    /// Return upper bound for `work_id`
    /// Determines how big the work registry has to be
    pub fn work_id_count(&self) -> usize {
        ExtWorkId::get_work_id_count(self.midstate_count)
    }

    fn init(&mut self) -> error::Result<()> {
        self.fifo.init()
    }

    fn new(hashboard_idx: usize, midstate_count: MidstateCount) -> error::Result<Self> {
        Ok(Self {
            fifo: WorkTxFifo::new(hashboard_idx)?,
            midstate_count,
        })
    }
}

pub struct CommandRxTx {
    fifo: CommandRxTxFifos,
    pub hashboard_idx: usize,
}

impl CommandRxTx {
    /// Serializes command into 32-bit words and submits it to the command TX FIFO
    ///
    /// * `wait` - when true, wait until all commands are sent
    pub async fn send_command(&self, cmd: Vec<u8>, wait: bool) {
        // invariant required by the IP core
        assert_eq!(
            cmd.len() & 0x3,
            0,
            "Control command length not aligned to 4 byte boundary!"
        );
        trace!("Sending Control Command {:x?}", cmd);
        for chunk in cmd.chunks(4) {
            self.fifo
                .write(u32::from_le_bytes(
                    chunk.try_into().expect("slice with incorrect length"),
                ))
                .await;
        }
        if wait {
            self.fifo.wait_tx_empty().await;
        }
    }

    /// Receive command response.
    /// Command responses are always 7 bytes long including checksum. Therefore, the reception
    /// has to be done in 2 steps with the following error handling:
    ///
    /// - A timeout when reading the first word is converted into an empty response.
    ///   The method propagates any error other than timeout
    /// - An error that occurs during reading the second word from the FIFO is propagated.
    pub async fn recv_response(&mut self, timeout: Duration) -> error::Result<Option<Vec<u8>>> {
        // assembled response
        let mut cmd_resp = Vec::new();

        // fetch first word of command response from IP core's fifo
        match self.fifo.read_with_timeout(timeout).await? {
            None => return Ok(None),
            Some(word1) => cmd_resp.extend_from_slice(&u32::to_le_bytes(word1)),
        }

        // fetch second word: getting timeout here is a hardware error
        match self.fifo.read_with_timeout(timeout).await? {
            None => Err(ErrorKind::Fifo(
                error::Fifo::TimedOut,
                "cmd RX fifo framing error".to_string(),
            ))?,
            Some(word2) => cmd_resp.extend_from_slice(&u32::to_le_bytes(word2)),
        }

        // build the response vector - drop the extra byte due to FIFO being 32-bit word based
        // and drop the checksum
        cmd_resp.truncate(6);
        Ok(Some(cmd_resp))
    }

    fn init(&mut self) -> error::Result<()> {
        self.fifo.init()
    }

    fn new(hashboard_idx: usize) -> error::Result<Self> {
        Ok(Self {
            fifo: CommandRxTxFifos::new(hashboard_idx)?,
            hashboard_idx,
        })
    }
}

/// Structure holding the `common` register block
pub struct Common {
    /// The `common` register block itself
    regs: uio_async::UioTypedMapping<ii_fpga_io_am1_s9::common::RegisterBlock>,
    /// Current midstate configuration
    midstate_count: MidstateCount,
    /// With which hashboard is this register block associated?
    /// This is required to print meaningful error messages.
    hashboard_idx: usize,
}

impl Common {
    /// Return build id (unix timestamp) of s9-io bitstream
    #[inline]
    fn get_build_id(&mut self) -> BuildId {
        BuildId(self.regs.build_id.read().bits())
    }

    /// Return version of FPGA bitstream
    #[inline]
    fn get_version(&mut self) -> Version {
        let ver = self.regs.version.read();
        let miner_type = match ver.miner_type().variant() {
            Variant::Val(t) => MinerType::Known(t),
            Variant::Res(i) => MinerType::Unknown(i as usize),
        };
        Version {
            miner_type,
            model: ver.model().bits() as usize,
            major: ver.major().bits() as usize,
            minor: ver.minor().bits() as usize,
            patch: ver.patch().bits() as usize,
        }
    }

    #[inline]
    pub fn enable_ip_core(&self) {
        self.regs.ctrl_reg.modify(|_, w| w.enable().bit(true));
    }

    #[inline]
    pub fn disable_ip_core(&self) {
        self.regs.ctrl_reg.modify(|_, w| w.enable().bit(false));
    }

    #[inline]
    pub fn set_ip_core_work_time(&self, work_time: u32) {
        self.regs.work_time.write(|w| unsafe { w.bits(work_time) });
    }

    #[inline]
    pub fn set_baud_clock_div(&self, baud_clock_div: u32) {
        self.regs
            .baud_reg
            .write(|w| unsafe { w.bits(baud_clock_div) });
    }

    /// XXX: not sure if we should leak the `ctrl_reg` type here
    /// (of course we shouldn't but who is the responsible for the translation?)
    /// Note: this function is not public because you ought to use `set_midstate_count`
    #[inline]
    fn set_ip_core_midstate_count(&self, midstate_count: MidstateCount) {
        let value = match midstate_count.to_bits() {
            0 => MIDSTATE_CNT_A::ONE,
            1 => MIDSTATE_CNT_A::TWO,
            2 => MIDSTATE_CNT_A::FOUR,
            _ => panic!("invalid midstate count logarithm"),
        };
        self.regs
            .ctrl_reg
            .modify(|_, w| w.midstate_cnt().variant(value));
    }

    fn check_version(&mut self) -> error::Result<()> {
        let version = self.get_version();
        let build_id = self.get_build_id();

        // check that there's something
        if !build_id.seems_legit() {
            Err(ErrorKind::Hashboard(
                self.hashboard_idx,
                "no s9_io bistream found".to_string(),
            ))?
        }

        // notify the user
        info!(
            "Hashboard {}: s9-io {} built on {}",
            self.hashboard_idx, version, build_id
        );

        // check it's the exact version
        if version != EXPECTED_S9IO_VERSION {
            Err(ErrorKind::UnexpectedVersion(
                "s9-io bitstream".to_string(),
                version.to_string(),
                EXPECTED_S9IO_VERSION.to_string(),
            ))?
        }
        Ok(())
    }

    pub fn set_midstate_count(&self) {
        self.set_ip_core_midstate_count(self.midstate_count);
    }

    fn init(&mut self) -> error::Result<()> {
        // reset ip core
        self.disable_ip_core();
        self.enable_ip_core();
        // check version
        self.check_version()?;
        Ok(())
    }

    fn new(hashboard_idx: usize, midstate_count: MidstateCount) -> error::Result<Self> {
        let uio = uio::Device::open(hashboard_idx, uio::Type::Common)?;
        Ok(Self {
            regs: uio.map()?,
            midstate_count,
            hashboard_idx,
        })
    }
}

/// Represents the whole IP core
pub struct Core {
    common_io: Common,
    command_io: CommandRxTx,
    work_rx_io: WorkRx,
    work_tx_io: WorkTx,
}

impl Core {
    /// Build a new IP core
    pub fn new(hashboard_idx: usize, midstate_count: MidstateCount) -> error::Result<Self> {
        Ok(Self {
            common_io: Common::new(hashboard_idx, midstate_count)?,
            command_io: CommandRxTx::new(hashboard_idx)?,
            work_rx_io: WorkRx::new(hashboard_idx, midstate_count)?,
            work_tx_io: WorkTx::new(hashboard_idx, midstate_count)?,
        })
    }

    /// Initialize the IP core and split it into components
    /// That way it's not possible to access un-initialized IO blocks
    pub fn init_and_split(mut self) -> error::Result<(Common, CommandRxTx, WorkRx, WorkTx)> {
        // common_io has to go first to reset the IP core
        self.common_io.init()?;

        // Initialize fifos
        self.command_io.init()?;
        self.work_rx_io.init()?;
        self.work_tx_io.init()?;

        Ok((
            self.common_io,
            self.command_io,
            self.work_rx_io,
            self.work_tx_io,
        ))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    /// Index of chain for testing (must exist and be defined in DTS)
    const TEST_CHAIN_INDEX: usize = 8;

    /// Test that we are able to construct HChainFifo instance
    #[test]
    fn test_fifo_initialization() {
        let core =
            Core::new(TEST_CHAIN_INDEX, MidstateCount::new(1)).expect("fifo construction failed");
        core.init_and_split().expect("fifo initialization failed");
    }
    /// This test verifies correct parsing of mining work solution for all multi-midstate
    /// configurations.
    /// The solution_word represents the second word of data provided that follows the nonce as
    /// provided by the FPGA IP core
    #[test]
    fn test_work_rx_response() {
        let word1 = 0xdead0666;
        let word2 = 0x98123502;
        struct ExpectedSolutionData {
            work_id: usize,
            midstate_idx: usize,
            solution_idx: usize,
            midstate_count: MidstateCount,
        };
        let expected_solution_data = [
            ExpectedSolutionData {
                work_id: 0x1235,
                midstate_idx: 0,
                solution_idx: 2,
                midstate_count: MidstateCount::new(1),
            },
            ExpectedSolutionData {
                work_id: 0x091a,
                midstate_idx: 1,
                solution_idx: 2,
                midstate_count: MidstateCount::new(2),
            },
            ExpectedSolutionData {
                work_id: 0x048d,
                midstate_idx: 1,
                solution_idx: 2,
                midstate_count: MidstateCount::new(4),
            },
        ];
        for (i, expected_solution_data) in expected_solution_data.iter().enumerate() {
            // The midstate configuration (ctrl_reg::MIDSTATE_CNT_W) doesn't implement a debug
            // trait. Therefore, we extract only those parts that can be easily displayed when a
            // test failed.
            let expected_data = (
                expected_solution_data.work_id,
                expected_solution_data.midstate_idx,
                expected_solution_data.solution_idx,
            );
            let resp = WorkRxResponse::from_hw(expected_solution_data.midstate_count, word1, word2);

            assert_eq!(resp.nonce, word1);
            assert_eq!(
                resp.work_id, expected_solution_data.work_id,
                "Invalid work ID, iteration: {}, test data: {:#06x?}",
                i, expected_data
            );
            assert_eq!(
                resp.midstate_idx, expected_solution_data.midstate_idx,
                "Invalid midstate index, iteration: {}, test data: {:#06x?}",
                i, expected_data
            );
            assert_eq!(
                resp.solution_idx, expected_solution_data.solution_idx,
                "Invalid solution index, iteration: {}, test data: {:#06x?}",
                i, expected_data
            );
        }
    }

    #[test]
    fn test_version_display() {
        let version = Version {
            miner_type: MinerType::Known(MINER_TYPE_A::ANTMINER),
            model: 9,
            major: 1,
            minor: 2,
            patch: 3,
        };

        assert_eq!(version.to_string(), "1.2.3 for Antminer S9");
        let version = Version {
            miner_type: MinerType::Unknown(10),
            model: 19,
            major: 1,
            minor: 2,
            patch: 3,
        };
        assert_eq!(version.to_string(), "1.2.3 for Unknown[10, 19]");
    }

    #[test]
    fn test_build_id_display() {
        let build_id = BuildId(0x5D8255F0);
        assert_eq!(build_id.to_string(), "2019-09-18 16:06:08 UTC");
    }

    /// Test work_time computation
    #[test]
    fn test_work_time_computation() {
        // you need to recalc this if you change asic diff or fpga freq
        assert_eq!(secs_to_fpga_ticks(0.00072592), 36296);
    }
}
