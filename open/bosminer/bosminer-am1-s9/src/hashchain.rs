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

//! Hashchain and related functionality

pub mod counters;
pub mod frequency;

pub use frequency::FrequencySettings;

use ii_logging::macros::*;

use ii_sensors::{self as sensor, Measurement};

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use futures::channel::mpsc;
use futures::lock::Mutex;
use ii_async_compat::futures;

use ii_async_compat::tokio;
use tokio::sync::watch;
use tokio::time::delay_for;

use crate::error::{self, ErrorKind};
use failure::ResultExt;

use bosminer::hal;
use bosminer::work;

use crate::bm1387::{self, ChipAddress, MidstateCount};
use crate::command;
use crate::gpio;
use crate::halt;
use crate::io;
use crate::monitor;
use crate::null_work;
use crate::power;
use crate::registry;
use crate::utils;

use once_cell::sync::OnceCell;

/// Import traits
use command::Interface;
use ii_bitcoin::MeetsTarget;
use packed_struct::PackedStruct;

/// Timing constants
const INACTIVATE_FROM_CHAIN_DELAY: Duration = Duration::from_millis(100);
/// Base delay quantum during hashboard initialization
const INIT_DELAY: Duration = Duration::from_secs(1);

/// Maximum number of chips on chain (inclusive).
/// Hard-limit is 64, any higher than that and you would have to change the addressing scheme.
pub const MAX_CHIPS_ON_CHAIN: usize = 63;
/// Number of chips to consider OK for initialization
pub const EXPECTED_CHIPS_ON_CHAIN: usize = 63;

/// Oscillator speed for all chips on S9 hash boards
pub const CHIP_OSC_CLK_HZ: usize = 25_000_000;

/// Exact value of the initial baud rate after reset of the hashing chips.
const INIT_CHIP_BAUD_RATE: usize = 115740;
/// Exact desired target baud rate when hashing at full speed (matches the divisor, too)
const TARGET_CHIP_BAUD_RATE: usize = 1562500;

/// Address of chip with connected temp sensor
const TEMP_CHIP: ChipAddress = ChipAddress::One(61);

/// Core address space size (it should be 114, but the addresses are non-consecutive)
const CORE_ADR_SPACE_SIZE: usize = 128;

/// Timeout for completion of haschain halt
const HALT_TIMEOUT: Duration = Duration::from_secs(30);

/// Pre-computed PLL for quick lookup
pub static PRECOMPUTED_PLL: OnceCell<bm1387::PllTable> = OnceCell::new();

/// Number of attempts to try read temperature again if it doesn't look right
const MAX_TEMP_REREAD_ATTEMPTS: usize = 3;

/// Temperature difference that is considerd to be a "sudden temperature change"
const MAX_SUDDEN_TEMPERATURE_JUMP: f32 = 12.0;

/// Maximum number of errors before temperature sensor is disabled
const MAX_SENSOR_ERRORS: usize = 10;

/// Type representing plug pin
#[derive(Clone)]
pub struct PlugPin {
    pin: gpio::PinIn,
}

impl PlugPin {
    pub fn open(gpio_mgr: &gpio::ControlPinManager, hashboard_idx: usize) -> error::Result<Self> {
        Ok(Self {
            pin: gpio_mgr
                .get_pin_in(gpio::PinInName::Plug(hashboard_idx))
                .context(ErrorKind::Hashboard(
                    hashboard_idx,
                    "failed to initialize plug pin".to_string(),
                ))?,
        })
    }

    pub fn hashboard_present(&self) -> error::Result<bool> {
        Ok(self.pin.is_high()?)
    }
}

/// Type representing reset pin
#[derive(Clone)]
pub struct ResetPin {
    pin: gpio::PinOut,
}

impl ResetPin {
    pub fn open(gpio_mgr: &gpio::ControlPinManager, hashboard_idx: usize) -> error::Result<Self> {
        Ok(Self {
            pin: gpio_mgr
                .get_pin_out(gpio::PinOutName::Rst(hashboard_idx))
                .context(ErrorKind::Hashboard(
                    hashboard_idx,
                    "failed to initialize reset pin".to_string(),
                ))?,
        })
    }

    pub fn enter_reset(&mut self) -> error::Result<()> {
        self.pin.set_low()?;
        Ok(())
    }

    pub fn exit_reset(&mut self) -> error::Result<()> {
        self.pin.set_high()?;
        Ok(())
    }
}

/// Represents solution from the hardware combined with difficulty
#[derive(Clone, Debug)]
pub struct Solution {
    /// Actual nonce
    nonce: u32,
    /// Index of a midstate that corresponds to the found nonce
    midstate_idx: usize,
    /// Index of a solution (if multiple were found)
    solution_idx: usize,
    /// Target to which was this solution solved
    target: ii_bitcoin::Target,
}

impl Solution {
    pub(super) fn from_hw_solution(hw: &io::Solution, target: ii_bitcoin::Target) -> Self {
        Self {
            nonce: hw.nonce,
            midstate_idx: hw.midstate_idx,
            solution_idx: hw.solution_idx,
            target,
        }
    }
}

impl hal::BackendSolution for Solution {
    #[inline]
    fn nonce(&self) -> u32 {
        self.nonce
    }

    #[inline]
    fn midstate_idx(&self) -> usize {
        self.midstate_idx
    }

    #[inline]
    fn solution_idx(&self) -> usize {
        self.solution_idx
    }

    #[inline]
    fn target(&self) -> &ii_bitcoin::Target {
        &self.target
    }
}

/// Helper method to calculate time to finish one piece of work
///
/// * `n_midstates` - number of midstates
/// * `pll_frequency` - frequency of chip in Hz
/// Return a number of seconds.
///
/// The formula for work_delay is:
///
///   work_delay = space_size_of_one_work / computation_speed; [sec, hashes, hashes_per_sec]
///
/// In our case it would be
///
///   work_delay = n_midstates * 2^32 / (freq * num_chips * cores_per_chip)
///
/// Unfortunately the space is not divided evenly, some nonces get never computed.
/// The current conjecture is that nonce space is divided by chip/core address,
/// ie. chip number 0x1a iterates all nonces 0x1axxxxxx. That's 6 bits of chip_address
/// and 7 bits of core_address. Putting it all together:
///
///   work_delay = n_midstates * num_chips * cores_per_chip * 2^(32 - 7 - 6) / (freq * num_chips * cores_per_chip)
///
/// Simplify:
///
///   work_delay = n_midstates * 2^19 / freq
///
/// Last but not least, we apply fudge factor of 0.9 and send work 11% faster to offset
/// delays when sending out/generating work/chips not getting proper work...:
///
///   work_delay = 0.9 * n_midstates * 2^19 / freq
fn calculate_work_delay_for_pll(n_midstates: usize, pll_frequency: usize) -> f64 {
    let space_size_per_core: u64 = 1 << 19;
    0.9 * (n_midstates as u64 * space_size_per_core) as f64 / pll_frequency as f64
}

/// Hash Chain Controller provides abstraction of the FPGA interface for operating hashing boards.
/// It is the user-space driver for the IP Core
///
/// Main responsibilities:
/// - memory mapping of the FPGA control interface
/// - mining work submission and solution processing
///
/// TODO: disable voltage controller via async `Drop` trait (which doesn't exist yet)
pub struct HashChain {
    /// Number of chips that have been detected
    pub chip_count: usize,
    /// Eliminates the need to query the IP core about the current number of configured midstates
    midstate_count: MidstateCount,
    /// ASIC difficulty
    pub asic_difficulty: usize,
    /// ASIC target (matches difficulty)
    asic_target: ii_bitcoin::Target,
    /// Voltage controller on this hashboard
    pub voltage_ctrl: Arc<power::Control>,
    /// Pin for resetting the hashboard
    reset_pin: ResetPin,
    hashboard_idx: usize,
    pub command_context: command::Context,
    pub common_io: io::Common,
    work_rx_io: Mutex<Option<io::WorkRx>>,
    work_tx_io: Mutex<Option<io::WorkTx>>,
    monitor_tx: mpsc::UnboundedSender<monitor::Message>,
    /// Do not send open-core work if this is true (some tests that test chip initialization may
    /// want to do this).
    pub(crate) disable_init_work: bool,
    /// channels through which temperature status is sent
    temperature_sender: Mutex<Option<watch::Sender<sensor::Temperature>>>,
    temperature_receiver: watch::Receiver<sensor::Temperature>,
    /// nonce counter
    pub counter: Arc<Mutex<counters::HashChain>>,
    /// halter to stop this hashchain
    pub halt_sender: Arc<halt::Sender>,
    /// we need to keep the halt receiver around, otherwise the "stop-notify" channel closes when chain ends
    #[allow(dead_code)]
    halt_receiver: halt::Receiver,
    /// Current hashchain settings
    pub frequency: Mutex<FrequencySettings>,
}

impl HashChain {
    /// Creates a new hashboard controller with memory mapped FPGA IP core
    ///
    /// * `gpio_mgr` - gpio manager used for producing pins required for hashboard control
    /// * `voltage_ctrl_backend` - communication backend for the voltage controller
    /// * `hashboard_idx` - index of this hashboard determines which FPGA IP core is to be mapped
    /// * `midstate_count` - see Self
    /// * `asic_difficulty` - to what difficulty set the hardware target filter
    pub fn new(
        reset_pin: ResetPin,
        plug_pin: PlugPin,
        voltage_ctrl_backend: Arc<power::I2cBackend>,
        hashboard_idx: usize,
        midstate_count: MidstateCount,
        asic_difficulty: usize,
        monitor_tx: mpsc::UnboundedSender<monitor::Message>,
    ) -> error::Result<Self> {
        let core = io::Core::new(hashboard_idx, midstate_count)?;
        // Unfortunately, we have to do IP core re-init here (but it should be OK, it's synchronous)
        let (common_io, command_io, work_rx_io, work_tx_io) = core.init_and_split()?;

        // check that the board is present
        if !plug_pin.hashboard_present()? {
            Err(ErrorKind::Hashboard(
                hashboard_idx,
                "not present".to_string(),
            ))?
        }

        // Precompute dividers, but do it just once
        PRECOMPUTED_PLL.get_or_init(|| bm1387::PllTable::build_pll_table(CHIP_OSC_CLK_HZ));

        // create temperature sending channel
        let (temperature_sender, temperature_receiver) =
            watch::channel(sensor::INVALID_TEMPERATURE_READING);

        // create halt notification channel
        let (halt_sender, halt_receiver) = halt::make_pair(HALT_TIMEOUT);

        Ok(Self {
            chip_count: 0,
            midstate_count,
            asic_difficulty,
            asic_target: ii_bitcoin::Target::from_pool_difficulty(asic_difficulty),
            voltage_ctrl: Arc::new(power::Control::new(voltage_ctrl_backend, hashboard_idx)),
            reset_pin,
            hashboard_idx,
            common_io,
            command_context: command::Context::new(command_io),
            work_rx_io: Mutex::new(Some(work_rx_io)),
            work_tx_io: Mutex::new(Some(work_tx_io)),
            monitor_tx,
            disable_init_work: false,
            temperature_sender: Mutex::new(Some(temperature_sender)),
            temperature_receiver,
            counter: Arc::new(Mutex::new(counters::HashChain::new(
                MAX_CHIPS_ON_CHAIN,
                asic_difficulty,
            ))),
            halt_sender,
            halt_receiver,
            frequency: Mutex::new(FrequencySettings::from_frequency(0)),
        })
    }

    pub fn current_temperature(&self) -> sensor::Temperature {
        self.temperature_receiver.borrow().clone()
    }

    pub(super) async fn take_work_rx_io(&self) -> io::WorkRx {
        self.work_rx_io
            .lock()
            .await
            .take()
            .expect("work-rx io missing")
    }

    pub(super) async fn take_work_tx_io(&self) -> io::WorkTx {
        self.work_tx_io
            .lock()
            .await
            .take()
            .expect("work-tx io missing")
    }

    /// Calculate work_time for this instance of HChain
    ///
    /// Returns number of ticks (suitable to be written to `WORK_TIME` register)
    ///
    /// TODO: move this function to `io.rs`
    #[inline]
    fn calculate_work_time(&self, max_pll_frequency: usize) -> u32 {
        io::secs_to_fpga_ticks(calculate_work_delay_for_pll(
            self.midstate_count.to_count(),
            max_pll_frequency,
        ))
    }

    /// Set work time depending on current PLL frequency
    ///
    /// This method sets work time so it's fast enough for `new_freq`
    async fn set_work_time(&self, new_freq: usize) {
        let new_work_time = self.calculate_work_time(new_freq);
        debug!("Using work time: {} for freq {}", new_work_time, new_freq);
        self.common_io.set_ip_core_work_time(new_work_time);
    }

    /// Helper method that initializes the FPGA IP core
    async fn ip_core_init(&mut self) -> error::Result<()> {
        // Configure IP core
        self.set_ip_core_baud_rate(INIT_CHIP_BAUD_RATE)?;
        self.common_io.set_midstate_count();

        Ok(())
    }

    /// Puts the board into reset mode and disables the associated IP core
    fn enter_reset(&mut self) -> error::Result<()> {
        self.common_io.disable_ip_core();
        // Warning: Reset pin DOESN'T reset the PIC. The PIC needs to be reset by other means.
        // Perform reset of the hashboard
        self.reset_pin.enter_reset()?;
        Ok(())
    }

    /// Leaves reset mode
    fn exit_reset(&mut self) -> error::Result<()> {
        self.reset_pin.exit_reset()?;
        self.common_io.enable_ip_core();
        Ok(())
    }

    /// Configures difficulty globally on all chips within the hashchain
    async fn set_asic_diff(&mut self, difficulty: usize) -> error::Result<()> {
        let tm_reg = bm1387::TicketMaskReg::new(difficulty as u32)?;
        trace!(
            "Setting ticket mask register for difficulty {}, value {:#010x?}",
            difficulty,
            tm_reg
        );
        self.command_context
            .write_register_readback(ChipAddress::All, &tm_reg)
            .await?;
        Ok(())
    }

    /// Reset hashboard and try to enumerate the chips.
    /// If not enough chips were found and `accept_less_chips` is not specified,
    /// treat it as error.
    async fn reset_and_enumerate_and_init(
        &mut self,
        accept_less_chips: bool,
        initial_frequency: &FrequencySettings,
    ) -> error::Result<()> {
        // Reset hashboard, toggle voltage
        info!("Resetting hash board");
        self.enter_reset()?;
        self.voltage_ctrl.disable_voltage().await?;
        delay_for(INIT_DELAY).await;
        self.voltage_ctrl.enable_voltage().await?;
        delay_for(INIT_DELAY * 2).await;
        self.exit_reset()?;
        delay_for(INIT_DELAY).await;

        // Enumerate chips
        info!("Starting chip enumeration");
        self.enumerate_chips().await?;

        // Figure out if we found enough chips
        info!("Discovered {} chips", self.chip_count);
        self.command_context.set_chip_count(self.chip_count).await;
        self.counter.lock().await.set_chip_count(self.chip_count);
        self.frequency.lock().await.set_chip_count(self.chip_count);

        // If we don't have full number of chips and we do not want incomplete chain, then raise
        // an error
        if self.chip_count < EXPECTED_CHIPS_ON_CHAIN && !accept_less_chips {
            Err(ErrorKind::ChipEnumeration(
                "Not enough chips on chain".into(),
            ))?;
        }

        // set PLL
        self.set_pll(initial_frequency).await?;

        // configure the hashing chain to operate at desired baud rate. Note that gate block is
        // enabled to allow continuous start of chips in the chain
        self.configure_hash_chain(TARGET_CHIP_BAUD_RATE, false, true)
            .await?;
        self.set_ip_core_baud_rate(TARGET_CHIP_BAUD_RATE)?;

        self.set_asic_diff(self.asic_difficulty).await?;

        Ok(())
    }

    /// Initializes the complete hashboard including enumerating all chips
    ///
    /// * if enumeration fails (for enumeration-related reason), try to retry
    ///   it up to pre-defined number of times
    /// * if less than 63 chips is found, retry the enumeration
    pub async fn init(
        &mut self,
        initial_frequency: &FrequencySettings,
        initial_voltage: power::Voltage,
        accept_less_chips: bool,
    ) -> error::Result<Arc<Mutex<registry::WorkRegistry<Solution>>>> {
        info!("Hashboard IP core initialized");
        self.voltage_ctrl
            .clone()
            .init(self.halt_receiver.clone())
            .await?;

        info!(
            "Initializing hash chain {}, (difficulty {})",
            self.hashboard_idx, self.asic_difficulty
        );
        self.ip_core_init().await?;

        // Enumerate chips
        self.reset_and_enumerate_and_init(accept_less_chips, initial_frequency)
            .await?;

        // Build shared work registry
        // TX fifo determines the size of work registry
        let work_registry = Arc::new(Mutex::new(registry::WorkRegistry::new(
            self.work_tx_io
                .lock()
                .await
                .as_ref()
                .expect("work-tx io missing")
                .work_id_count(),
        )));

        // send opencore work (at high voltage) unless someone disabled it
        if !self.disable_init_work {
            self.send_init_work(work_registry.clone()).await;
        }

        // lower voltage to working level
        self.voltage_ctrl
            .set_voltage(initial_voltage)
            .await
            .expect("lowering voltage failed");

        // return work registry we created
        Ok(work_registry)
    }

    /// Detects the number of chips on the hashing chain and assigns an address to each chip
    async fn enumerate_chips(&mut self) -> error::Result<()> {
        // Enumerate all chips (broadcast read address register request)
        let responses = self
            .command_context
            .read_register::<bm1387::GetAddressReg>(ChipAddress::All)
            .await?;

        // Reset chip count (we might get called multiple times)
        self.chip_count = 0;
        // Check if are responses meaningful
        for (address, addr_reg) in responses.iter().enumerate() {
            if addr_reg.chip_rev != bm1387::CHIP_REV_BM1387 {
                Err(ErrorKind::ChipEnumeration(format!(
                    "unexpected revision of chip {} (expected: {:#x?} received: {:#x?})",
                    address,
                    bm1387::CHIP_REV_BM1387,
                    addr_reg.chip_rev,
                )))?
            }
            self.chip_count += 1;
        }
        if self.chip_count > MAX_CHIPS_ON_CHAIN {
            Err(ErrorKind::ChipEnumeration(format!(
                "detected {} chips, expected maximum {} chips on one chain. Possibly a hardware issue?",
                self.chip_count,
                MAX_CHIPS_ON_CHAIN,
            )))?
        }
        if self.chip_count == 0 {
            Err(ErrorKind::ChipEnumeration(
                "no chips detected on the current chain".to_string(),
            ))?
        }

        // Set all chips to be offline before address assignment. This is important so that each
        // chip after initially accepting the address will pass on further addresses down the chain
        let inactivate_from_chain_cmd = bm1387::InactivateFromChainCmd::new().pack();
        // make sure all chips receive inactivation request
        for _ in 0..3 {
            self.command_context
                .send_raw_command(inactivate_from_chain_cmd.to_vec(), false)
                .await;
            delay_for(INACTIVATE_FROM_CHAIN_DELAY).await;
        }

        // Assign address to each chip
        for i in 0..self.chip_count {
            let cmd = bm1387::SetChipAddressCmd::new(ChipAddress::One(i));
            self.command_context
                .send_raw_command(cmd.pack().to_vec(), false)
                .await;
        }

        Ok(())
    }

    /// Loads PLL register with a starting value
    ///
    /// WARNING: you have to take care of `set_work_time` yourself
    async fn set_chip_pll(&self, chip_addr: ChipAddress, freq: usize) -> error::Result<()> {
        // convert frequency to PLL setting register
        let pll = PRECOMPUTED_PLL
            .get()
            .expect("BUG: PLL table not initialized")
            .lookup(freq)?;

        info!(
            "chain {}: setting frequency {} MHz on {:?} (error {} MHz)",
            self.hashboard_idx,
            freq / 1_000_000,
            chip_addr,
            ((freq as f64) - (pll.frequency as f64)).abs() / 1_000_000.0,
        );

        // NOTE: When PLL register is read back, it is or-ed with 0x8000_0000, not sure why.
        //  Avoid reading it back to prevent disappointment.
        self.command_context
            .write_register(chip_addr, &pll.reg)
            .await?;

        Ok(())
    }

    /// Load PLL register of all chips
    ///
    /// Takes care of adjusting `work_time`
    pub async fn set_pll(&self, frequency: &FrequencySettings) -> error::Result<()> {
        // TODO: find a better way - how to communicate with frequency setter how many chips we have?
        assert!(frequency.chip.len() >= self.chip_count);

        // Check if the frequencies are identical
        if frequency.min() == frequency.max() {
            // Update them in one go
            self.set_chip_pll(ChipAddress::All, frequency.chip[0])
                .await?;
        } else {
            // Update chips one-by-one
            for i in 0..self.chip_count {
                let new_freq = self.frequency.lock().await.chip[i];
                if new_freq != frequency.chip[i] {
                    self.set_chip_pll(ChipAddress::One(i), new_freq).await?;
                }
            }
        }

        // Update worktime
        self.set_work_time(frequency.max()).await;

        // Remember what frequencies are set
        let mut cur_frequency = self.frequency.lock().await;
        for i in 0..self.chip_count {
            cur_frequency.chip[i] = frequency.chip[i];
        }

        Ok(())
    }

    /// Configure all chips in the hash chain
    ///
    /// This method programs the MiscCtrl register of each chip in the hash chain.
    ///
    /// * `baud_rate` - desired communication speed
    /// * `not_set_baud` - the baud clock divisor is calculated, however, each chip will ignore
    /// its value. This is used typically when gate_block is enabled.
    /// * `gate_block` - allows gradual startup of the chips in the chain as they keep receiving
    /// special 'null' job. See bm1387::MiscCtrlReg::gate_block for details
    ///
    /// Returns actual baud rate that has been set on the chips or an error
    /// @todo Research the exact use case of 'not_set_baud' in conjunction with gate_block
    async fn configure_hash_chain(
        &mut self,
        baud_rate: usize,
        not_set_baud: bool,
        gate_block: bool,
    ) -> error::Result<usize> {
        let (baud_clock_div, actual_baud_rate) = utils::calc_baud_clock_div(
            baud_rate,
            CHIP_OSC_CLK_HZ,
            bm1387::CHIP_OSC_CLK_BASE_BAUD_DIV,
        )?;
        info!(
            "Setting Hash chain baud rate @ requested: {}, actual: {}, divisor {:#04x}",
            baud_rate, actual_baud_rate, baud_clock_div
        );
        // Each chip is always configured with inverted clock
        let ctl_reg =
            bm1387::MiscCtrlReg::new(not_set_baud, true, baud_clock_div, gate_block, true)?;
        // Do not read back the MiscCtrl register when setting baud rate: it will result
        // in serial speed mismatch and nothing being read.
        self.command_context
            .write_register(ChipAddress::All, &ctl_reg)
            .await?;
        Ok(actual_baud_rate)
    }

    /// This method only changes the communication speed of the FPGA IP core with the chips.
    ///
    /// Note: change baud rate of the FPGA is only desirable as a step after all chips in the
    /// chain have been reconfigured for a different speed, too.
    fn set_ip_core_baud_rate(&self, baud: usize) -> error::Result<()> {
        let (baud_clock_div, actual_baud_rate) =
            utils::calc_baud_clock_div(baud, io::F_CLK_SPEED_HZ, io::F_CLK_BASE_BAUD_DIV)?;
        info!(
            "Setting IP core baud rate @ requested: {}, actual: {}, divisor {:#04x}",
            baud, actual_baud_rate, baud_clock_div
        );

        self.common_io.set_baud_clock_div(baud_clock_div as u32);
        Ok(())
    }

    pub fn get_chip_count(&self) -> usize {
        self.chip_count
    }

    /// Initialize cores by sending open-core work with correct nbits to each core
    async fn send_init_work(
        &mut self,
        work_registry: Arc<Mutex<registry::WorkRegistry<Solution>>>,
    ) {
        // Each core gets one work
        const NUM_WORK: usize = bm1387::NUM_CORES_ON_CHIP;
        trace!(
            "Sending out {} pieces of dummy work to initialize chips",
            NUM_WORK
        );
        let midstate_count = self.midstate_count.to_count();
        let mut work_tx_io = self.work_tx_io.lock().await;
        let tx_fifo = work_tx_io.as_mut().expect("tx fifo missing");
        for _ in 0..NUM_WORK {
            let work = &null_work::prepare_opencore(true, midstate_count);
            // store work to registry as "initial work" so that later we can properly ignore
            // solutions
            let work_id = work_registry.lock().await.store_work(work.clone(), true);
            tx_fifo.wait_for_room().await.expect("wait for tx room");
            tx_fifo.send_work(&work, work_id).expect("send work");
        }
    }

    /// This task picks up work from frontend (via generator), saves it to
    /// registry (to pair with `Assignment` later) and sends it out to hw.
    /// It makes sure that TX fifo is empty before requesting work from
    /// generator.
    /// It exits when generator returns `None`.
    async fn work_tx_task(
        work_registry: Arc<Mutex<registry::WorkRegistry<Solution>>>,
        mut tx_fifo: io::WorkTx,
        mut work_generator: work::Generator,
    ) {
        loop {
            tx_fifo.wait_for_room().await.expect("wait for tx room");
            let work = work_generator.generate().await;
            match work {
                None => return,
                Some(work) => {
                    // assign `work_id` to `work`
                    let work_id = work_registry.lock().await.store_work(work.clone(), false);
                    // send work is synchronous
                    tx_fifo.send_work(&work, work_id).expect("send work");
                }
            }
        }
    }

    /// This task receives solutions from hardware, looks up `Assignment` in
    /// registry (under `work_id` got from FPGA), pairs them together and
    /// sends them back to frontend (via `solution_sender`).
    /// If solution is duplicated, it gets dropped (and errors stats incremented).
    /// It prints warnings when solution doesn't hit ASIC target.
    /// TODO: this task is not very platform dependent, maybe move it somewhere else?
    /// TODO: figure out when and how to stop this task
    async fn solution_rx_task(
        self: Arc<Self>,
        work_registry: Arc<Mutex<registry::WorkRegistry<Solution>>>,
        mut rx_fifo: io::WorkRx,
        solution_sender: work::SolutionSender,
        counter: Arc<Mutex<counters::HashChain>>,
    ) {
        // solution receiving/filtering part
        loop {
            let (rx_fifo_out, hw_solution) =
                rx_fifo.recv_solution().await.expect("recv solution failed");
            rx_fifo = rx_fifo_out;
            let work_id = hw_solution.hardware_id;
            let solution = Solution::from_hw_solution(&hw_solution, self.asic_target);
            let mut work_registry = work_registry.lock().await;

            let work = work_registry.find_work(work_id as usize);
            match work {
                Some(work_item) => {
                    // ignore solutions coming from initial work
                    if work_item.initial_work {
                        continue;
                    }
                    let core_addr = bm1387::CoreAddress::new(solution.nonce);
                    let status = work_item.insert_solution(solution);

                    // work item detected a new unique solution, we will push it for further processing
                    if let Some(unique_solution) = status.unique_solution {
                        if !status.duplicate {
                            let hash = unique_solution.hash();
                            if !hash.meets(unique_solution.backend_target()) {
                                trace!("Solution from hashchain not hitting ASIC target; {}", hash);
                                counter.lock().await.add_error(core_addr);
                            } else {
                                counter.lock().await.add_valid(core_addr);
                            }
                            solution_sender.send(unique_solution);
                        }
                    }
                    if status.duplicate {
                        counter.lock().await.add_error(core_addr);
                    }
                    if status.mismatched_nonce {
                        counter.lock().await.add_error(core_addr);
                    }
                }
                None => {
                    info!(
                        "No work present for solution, ID:{:#x} {:#010x?}",
                        work_id, solution
                    );
                }
            }
        }
    }

    async fn try_to_initialize_sensor(
        command_context: command::Context,
    ) -> error::Result<Box<dyn sensor::Sensor>> {
        // construct I2C bus via command interface
        let i2c_bus = bm1387::i2c::Bus::new_and_init(command_context, TEMP_CHIP)
            .await
            .with_context(|_| ErrorKind::Sensors("bus construction failed".into()))?;

        // try to probe sensor
        let sensor = sensor::probe_i2c_sensors(i2c_bus)
            .await
            .map_err(|e| error::Error::from(e))
            .with_context(|_| ErrorKind::Sensors("error when probing sensors".into()))?;

        // did we find anything?
        let mut sensor = match sensor {
            Some(sensor) => sensor,
            None => Err(ErrorKind::Sensors("no sensors found".into()))?,
        };

        // try to initialize sensor
        sensor
            .init()
            .await
            .map_err(|e| error::Error::from(e))
            .with_context(|_| ErrorKind::Sensors("failed to initialize sensors".into()))?;

        // done
        Ok(sensor)
    }

    /// This task just pings watchdog with no sensor readings
    async fn no_sensor_task(&self) {
        loop {
            // Send heartbeat to monitor with "unknown" temperature
            self.monitor_tx
                .unbounded_send(monitor::Message::Running(
                    sensor::INVALID_TEMPERATURE_READING,
                ))
                .expect("send failed");

            delay_for(bosminer_antminer::monitor::TEMP_UPDATE_INTERVAL).await;
        }
    }

    fn max_temp(t: sensor::Temperature) -> Option<f32> {
        let local: Option<f32> = t.local.into();
        let remote: Option<f32> = t.remote.into();
        match local {
            Some(t1) => match remote {
                Some(t2) => Some(t1.max(t2)),
                None => local,
            },
            None => remote,
        }
    }

    /// Monitor watchdog task.
    /// This task sends periodically ping to monitor task. It also tries to read temperature.
    async fn monitor_watchdog_temp_task(self: Arc<Self>) {
        // fetch hashboard idx
        info!(
            "Monitor watchdog temperature task started for hashchain {}",
            self.hashboard_idx
        );

        // take out temperature sender channel
        let temperature_sender = self
            .temperature_sender
            .lock()
            .await
            .take()
            .expect("BUG: temperature sender missing");

        // Wait some time before trying to initialize temperature controller
        // (Otherwise RX queue might be clogged with initial work and we will not get any replies)
        //
        // TODO: we should implement a more robust mechanism that controls access to the I2C bus of
        // a hashing chip only if the hashchain allows it (hashchain is in operation etc.)
        delay_for(Duration::from_secs(5)).await;

        // Try to probe sensor
        // This may fail - in which case we put `None` into `sensor`
        let mut sensor = match Self::try_to_initialize_sensor(self.command_context.clone())
            .await
            .with_context(|_| ErrorKind::Hashboard(self.hashboard_idx, "sensor error".into()))
            .map_err(|e| e.into())
        {
            error::Result::Err(e) => {
                error!(
                    "Hashchain {}: Sensor probing failed: {}",
                    self.hashboard_idx, e
                );
                return self.no_sensor_task().await;
            }
            error::Result::Ok(sensor) => sensor,
        };

        // Compare temperature to previous to check if sudden temperature change occured
        let mut last_max_temp: Option<f32> = None;
        let mut try_again = MAX_TEMP_REREAD_ATTEMPTS;
        let mut error_counter = 0.0;

        // "Watchdog" loop that pings monitor every some seconds
        loop {
            // Read temperature sensor
            let temp = match sensor
                .read_temperature()
                .await
                .map_err(|e| error::Error::from(e))
                .with_context(|_| {
                    ErrorKind::Hashboard(self.hashboard_idx, "temperature read fail".into())
                })
                .map_err(|e| e.into())
            {
                error::Result::Ok(temp) => {
                    info!(
                        "Hashchain {}: Measured temperature: {:?}, errors average: {:.1}",
                        self.hashboard_idx, temp, error_counter
                    );
                    temp
                }
                error::Result::Err(e) => {
                    error!(
                        "Hashchain {}: Sensor temperature read failed: {}",
                        self.hashboard_idx, e
                    );
                    sensor::INVALID_TEMPERATURE_READING
                }
            };
            match temp.remote {
                Measurement::OpenCircuit
                | Measurement::ShortCircuit
                | Measurement::InvalidReading => error_counter += 1.0,
                _ => {}
            }

            let max_temp = Self::max_temp(temp.clone());
            // Check for weird temperatures
            if try_again > 0 {
                // Both previous and current temperature must exist
                if let Some(max_t) = max_temp {
                    if let Some(old_max_t) = last_max_temp {
                        if (max_t - old_max_t).abs() >= MAX_SUDDEN_TEMPERATURE_JUMP {
                            warn!(
                                "Hashchain {}: Temperature suddenly jumped from {} to {}, reading again",
                                self.hashboard_idx, old_max_t, max_t
                            );
                            error_counter += 1.0;

                            // Do not send anything out yet, just wait a bit and then try to read
                            // the temperature again
                            delay_for(Duration::from_millis(200)).await;
                            try_again -= 1;
                            continue;
                        }
                    }
                }
            }

            // If there's too many errors, just disable this sensor
            if error_counter >= MAX_SENSOR_ERRORS as f64 {
                error!(
                    "Hashchain {}: Too many sensor errors, disabling sensor",
                    self.hashboard_idx
                );
                return self.no_sensor_task().await;
            }
            // Decay 30% of errors an hour: update it every TEMP_UPDATE_INTERVAL seconds, so one hour is
            // `error_counter * 0.9995^(3600/5) = error_counter * 0.70`
            error_counter *= 0.9995;

            // Update last temp and reset tries
            if max_temp.is_some() {
                last_max_temp = max_temp;
            }
            try_again = MAX_TEMP_REREAD_ATTEMPTS;

            // Broadcast
            temperature_sender
                .broadcast(temp.clone())
                .expect("temp broadcast failed");

            // Send heartbeat to monitor
            self.monitor_tx
                .unbounded_send(monitor::Message::Running(temp))
                .expect("send failed");

            delay_for(bosminer_antminer::monitor::TEMP_UPDATE_INTERVAL).await;
        }
    }

    /// Hashrate monitor task
    /// Fetch perodically information about hashrate
    #[allow(dead_code)]
    async fn hashrate_monitor_task(self: Arc<Self>) {
        info!("Hashrate monitor task started");
        loop {
            delay_for(Duration::from_secs(5)).await;

            let responses = self
                .command_context
                .read_register::<bm1387::HashrateReg>(ChipAddress::All)
                .await
                .expect("reading hashrate_reg failed");

            let mut sum = 0;
            for (chip_address, hashrate_reg) in responses.iter().enumerate() {
                trace!(
                    "chip {} hashrate {} GHash/s",
                    chip_address,
                    hashrate_reg.hashrate() as f64 / 1e9
                );
                sum += hashrate_reg.hashrate() as u128;
            }
            info!("Total chip hashrate {} GH/s", sum as f64 / 1e9);
        }
    }

    pub async fn start(
        self: Arc<Self>,
        work_generator: work::Generator,
        solution_sender: work::SolutionSender,
        work_registry: Arc<Mutex<registry::WorkRegistry<Solution>>>,
    ) {
        // spawn tx task
        let tx_fifo = self.take_work_tx_io().await;
        self.halt_receiver
            .register_client("work-tx".into())
            .await
            .spawn(Self::work_tx_task(
                work_registry.clone(),
                tx_fifo,
                work_generator,
            ));

        // spawn rx task
        let rx_fifo = self.take_work_rx_io().await;
        self.halt_receiver
            .register_client("work-rx".into())
            .await
            .spawn(Self::solution_rx_task(
                self.clone(),
                work_registry.clone(),
                rx_fifo,
                solution_sender,
                self.counter.clone(),
            ));

        // spawn hashrate monitor
        // Disabled until we found a use for this
        /*
        self.halt_receiver
            .register_client("hashrate monitor".into())
            .await
            .spawn(Self::hashrate_monitor_task(self.clone()));
        */

        // spawn temperature monitor
        self.halt_receiver
            .register_client("temperature monitor".into())
            .await
            .spawn(Self::monitor_watchdog_temp_task(self.clone()));
    }

    pub async fn reset_counter(&self) {
        self.counter.lock().await.reset();
    }

    pub async fn snapshot_counter(&self) -> counters::HashChain {
        self.counter.lock().await.snapshot()
    }

    pub async fn get_frequency(&self) -> FrequencySettings {
        self.frequency.lock().await.clone()
    }

    pub async fn get_voltage(&self) -> power::Voltage {
        self.voltage_ctrl
            .get_current_voltage()
            .await
            .expect("BUG: no voltage on hashchain")
    }
}

impl fmt::Debug for HashChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash Board {}", self.hashboard_idx)
    }
}

impl fmt::Display for HashChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash Board {}", self.hashboard_idx)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// Test work_time computation
    #[test]
    fn test_work_time_computation() {
        // you need to recalc this if you change asic diff or fpga freq
        assert_eq!(
            io::secs_to_fpga_ticks(calculate_work_delay_for_pll(1, 650_000_000)),
            36296
        );
    }
}
