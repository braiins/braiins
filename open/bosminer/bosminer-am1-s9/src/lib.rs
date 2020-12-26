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
#![recursion_limit = "256"]

mod cgminer;
pub mod config;
pub mod error;
pub mod gpio;
pub mod hashchain;
pub mod hooks;
pub mod power;

#[cfg(test)]
pub mod test;

use ii_logging::macros::*;

use ii_sensors as sensor;

use bosminer::async_trait;
use bosminer::hal::{self, BackendConfig as _};
use bosminer::node;
use bosminer::stats;
use bosminer::work;

/// TODO: make pub uses non-pub and fix it in dependant crates
pub use bosminer_antminer::bm1387;
use bosminer_antminer::bm1387::command;
pub use bosminer_antminer::fan;
pub use bosminer_antminer::halt;
use bosminer_antminer::io;
pub use bosminer_antminer::monitor;
use bosminer_antminer::null_work;
use bosminer_antminer::registry;
use bosminer_antminer::utils;

use bosminer_macros::WorkSolverNode;

use std::fmt;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use futures::channel::mpsc;
use futures::lock::Mutex;
use futures::stream::StreamExt;
use ii_async_compat::futures;

use ii_async_compat::tokio;
use tokio::sync::watch;
use tokio::time::delay_for;

/// Time to wait between successive hashboard initialization attempts
const ENUM_RETRY_DELAY: Duration = Duration::from_secs(10);
/// How many times to retry the enumeration
const ENUM_RETRY_COUNT: usize = 10;

/// Timeout for completion of haschain halt
const HALT_TIMEOUT: Duration = Duration::from_secs(30);

/// Power type alias
/// TODO: Implement it as a proper type (not just alias)
pub type Power = usize;

/// Token is type that represents exclusive access to the Manager. When token is dropped, so is
/// the exclusive access.
/// Of course this access can be bypassed in various ways, for example, `Ctrl-C` or "temperature
/// too high" can initiate the shutdown of a hashchain despite it's being locked.
#[derive(Debug)]
pub struct Token {
    manager: Arc<Manager>,
}

impl Token {
    fn new(manager: Arc<Manager>) -> Self {
        Token { manager }
    }

    fn manager(&self) -> Arc<Manager> {
        self.manager.clone()
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        // remove ownership in case we are dropped
        self.manager
            .owned_by
            .lock()
            .expect("BUG: token lock failed")
            .take()
            .expect("BUG: manager was unlocked but token was in circulation");
    }
}

#[derive(Debug)]
pub struct StoppedChain {
    token: Token,
    pub manager: Arc<Manager>,
}

impl StoppedChain {
    pub fn from_token(token: Token) -> Self {
        StoppedChain {
            manager: token.manager(),
            token,
        }
    }

    pub fn from_running(running_chain: RunningChain) -> Self {
        let RunningChain { token, .. } = running_chain;
        Self::from_token(token)
    }

    pub async fn start(
        self,
        initial_frequency: &hashchain::FrequencySettings,
        initial_voltage: power::Voltage,
        asic_difficulty: usize,
    ) -> Result<RunningChain, (Self, error::Error)> {
        // if miner initialization fails, retry
        let mut tries_left = ENUM_RETRY_COUNT;

        loop {
            info!(
                "Registering hashboard {} with monitor",
                self.manager.hashboard_idx
            );

            // Start this hashchain
            // If we've already exhausted half of our tries, then stop worrying about having
            // less chips than expected (63).
            match self
                .manager
                .attempt_start_chain(
                    tries_left <= ENUM_RETRY_COUNT / 2,
                    initial_frequency,
                    initial_voltage,
                    asic_difficulty,
                )
                .await
            {
                // start successful
                Ok(_) => {
                    // we've started the hashchain
                    // create a `Running` tape and be gone
                    return Ok(RunningChain::from_stopped(self).await);
                }
                // start failed
                Err(e) => {
                    error!("Chain {} start failed: {}", self.manager.hashboard_idx, e);

                    // retry if possible
                    if tries_left == 0 {
                        error!("No tries left");
                        return Err((self, e.into()));
                    } else {
                        tries_left -= 1;
                        // TODO: wait with locks unlocked()! Otherwise no-one can halt the miner
                        // This is not possible with current lock design, but fix this ASAP!
                        delay_for(ENUM_RETRY_DELAY).await;
                        info!("Retrying chain {} start...", self.manager.hashboard_idx);
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct RunningChain {
    token: Token,
    pub manager: Arc<Manager>,
    pub start_id: usize,
    pub asic_difficulty: usize,
}

impl RunningChain {
    pub async fn from_token(token: Token) -> Self {
        let manager = token.manager();
        let inner = manager.inner.lock().await;
        let hash_chain = inner
            .hash_chain
            .as_ref()
            .expect("BUG: hashchain is not running");
        RunningChain {
            asic_difficulty: hash_chain.asic_difficulty,
            start_id: inner.start_count,
            token,
            manager: manager.clone(),
        }
    }

    pub async fn from_stopped(stopped_chain: StoppedChain) -> Self {
        let StoppedChain { token, .. } = stopped_chain;
        Self::from_token(token).await
    }

    pub async fn stop(self) -> StoppedChain {
        self.manager.stop_chain(false).await;
        StoppedChain::from_running(self)
    }

    /// TODO: for the love of god use macros or something
    pub async fn get_frequency(&self) -> hashchain::FrequencySettings {
        let inner = self.manager.inner.lock().await;
        inner
            .hash_chain
            .as_ref()
            .expect("BUG: hashchain is not running")
            .get_frequency()
            .await
    }

    /// TODO: for the love of god use macros or something
    pub async fn get_voltage(&self) -> power::Voltage {
        let inner = self.manager.inner.lock().await;
        inner
            .hash_chain
            .as_ref()
            .expect("BUG: hashchain is not running")
            .get_voltage()
            .await
    }

    pub async fn set_frequency(
        &self,
        frequency: &hashchain::FrequencySettings,
    ) -> error::Result<()> {
        let inner = self.manager.inner.lock().await;
        inner
            .hash_chain
            .as_ref()
            .expect("BUG: hashchain is not running")
            .set_pll(frequency)
            .await
    }

    pub async fn set_voltage(&self, voltage: power::Voltage) -> error::Result<()> {
        let inner = self.manager.inner.lock().await;
        inner
            .hash_chain
            .as_ref()
            .expect("BUG: hashchain is not running")
            .voltage_ctrl
            .set_voltage(voltage)
            .await
    }

    pub async fn reset_counter(&self) {
        self.manager
            .inner
            .lock()
            .await
            .hash_chain
            .as_ref()
            .expect("not running")
            .reset_counter()
            .await;
    }

    pub async fn snapshot_counter(&self) -> hashchain::counters::HashChain {
        self.manager
            .inner
            .lock()
            .await
            .hash_chain
            .as_ref()
            .expect("not running")
            .snapshot_counter()
            .await
    }

    pub async fn current_temperature(&self) -> sensor::Temperature {
        self.manager
            .inner
            .lock()
            .await
            .hash_chain
            .as_ref()
            .expect("not running")
            .current_temperature()
    }

    /// Check from `Monitor` status message if miner is hot enough
    /// Also: this will break if there are no temperature sensors
    fn preheat_ok(status: monitor::Status) -> bool {
        const PREHEAT_TEMP_EPSILON: f32 = 2.0;
        let target_temp;
        // check if we are in PID mode, otherwise return `true`
        match status.config.fan_config {
            // Can't preheat if we are not controlling fans
            None => return true,
            Some(fan_config) => match fan_config.mode {
                monitor::FanControlMode::TargetTemperature(t) => target_temp = t,
                _ => return true,
            },
        }
        info!(
            "Preheat: waiting for target temperature: {}, current temperature: {:?}",
            target_temp, status.input_temperature
        );
        // we are in PID mode, check if temperature is OK
        match status.input_temperature {
            monitor::ChainTemperature::Ok(t) => {
                if t >= target_temp || target_temp - t < PREHEAT_TEMP_EPSILON {
                    info!("Preheat: temperature {} is hot enough", t);
                    return true;
                }
            }
            _ => (),
        }
        return false;
    }

    /// Wait for hashboard to reach PID-defined temperature (or higher)
    /// If monitor isn't in PID mode then this is effectively no-op.
    /// Wait at most predefined number of seconds to avoid any kind of dead-locks.
    ///
    /// Note: we have to lock it on the inside, because otherwise we would hold lock on hashchain
    /// manager and prevent shutdown from happening.
    pub async fn wait_for_preheat(&self) {
        const MAX_PREHEAT_DELAY: u64 = 180;

        let mut status_receiver = self.manager.status_receiver.clone();
        // wait for status from monitor
        let started = Instant::now();
        // TODO: wrap `status_receiver` into some kind of API
        while let Some(status) = status_receiver.next().await {
            // take just non-empty status messages
            if let Some(status) = status {
                if Self::preheat_ok(status) {
                    break;
                }
            }
            // in case we are waiting for too long, just skip preheat
            if Instant::now().duration_since(started).as_secs() >= MAX_PREHEAT_DELAY {
                info!("Preheat: waiting too long to heat-up, skipping preheat");
                return;
            }
        }
    }
}

pub enum ChainStatus {
    Running(RunningChain),
    Stopped(StoppedChain),
}

impl ChainStatus {
    pub fn expect_stopped(self) -> StoppedChain {
        match self {
            Self::Stopped(s) => s,
            _ => panic!("BUG: expected stopped chain"),
        }
    }
}

pub struct ManagerInner {
    pub hash_chain: Option<Arc<hashchain::HashChain>>,
    /// Each (attempted) hashchain start increments this counter by 1
    pub start_count: usize,
}

/// Hashchain manager that can start and stop instances of hashchain
/// TODO: split this structure into outer and inner part so that we can
/// deal with locking issues on the inside.
#[derive(WorkSolverNode)]
pub struct Manager {
    #[member_work_solver_stats]
    work_solver_stats: stats::BasicWorkSolver,
    pub hashboard_idx: usize,
    work_generator: work::Generator,
    solution_sender: work::SolutionSender,
    plug_pin: hashchain::PlugPin,
    reset_pin: hashchain::ResetPin,
    voltage_ctrl_backend: Arc<power::I2cBackend>,
    midstate_count: bm1387::MidstateCount,
    /// channel to report to the monitor
    monitor_tx: mpsc::UnboundedSender<monitor::Message>,
    /// TODO: wrap this type in a structure (in Monitor)
    pub status_receiver: watch::Receiver<Option<monitor::Status>>,
    owned_by: StdMutex<Option<&'static str>>,
    pub inner: Mutex<ManagerInner>,
    pub chain_config: config::ResolvedChainConfig,
}

impl Manager {
    /// Acquire stopped or running chain
    pub async fn acquire(
        self: Arc<Self>,
        owner_name: &'static str,
    ) -> Result<ChainStatus, &'static str> {
        // acquire ownership of the hashchain
        {
            let mut owned_by = self.owned_by.lock().expect("BUG: failed to lock mutex");
            if let Some(already_owned_by) = *owned_by {
                return Err(already_owned_by);
            }
            owned_by.replace(owner_name);
        }
        // Create a `Chain` instance. If it's dropped, the ownership reverts back to `Manager`
        let token = Token::new(self.clone());
        Ok(if self.inner.lock().await.hash_chain.is_some() {
            ChainStatus::Running(RunningChain::from_token(token).await)
        } else {
            ChainStatus::Stopped(StoppedChain::from_token(token))
        })
    }

    /// Initialize and start mining on hashchain
    /// TODO: this function is private and should be called only from `Stopped`
    async fn attempt_start_chain(
        &self,
        accept_less_chips: bool,
        initial_frequency: &hashchain::FrequencySettings,
        initial_voltage: power::Voltage,
        asic_difficulty: usize,
    ) -> error::Result<()> {
        // lock inner to guarantee atomicity of hashchain start
        let mut inner = self.inner.lock().await;

        // register us with monitor
        self.monitor_tx
            .unbounded_send(monitor::Message::On)
            .expect("BUG: send failed");

        // check that we hadn't started some other (?) way
        // TODO: maybe we should throw an error instead
        assert!(inner.hash_chain.is_none());

        // Increment start counter
        inner.start_count += 1;

        // make us a hash chain
        let mut hash_chain = hashchain::HashChain::new(
            self.reset_pin.clone(),
            self.plug_pin.clone(),
            self.voltage_ctrl_backend.clone(),
            self.hashboard_idx,
            self.midstate_count,
            asic_difficulty,
            self.monitor_tx.clone(),
        )
        .expect("BUG: hashchain instantiation failed");

        // initialize it
        let work_registry = match hash_chain
            .init(initial_frequency, initial_voltage, accept_less_chips)
            .await
        {
            Err(e) => {
                // halt is required to stop voltage heart-beat task
                hash_chain.halt_sender.clone().send_halt().await;
                // deregister us
                self.monitor_tx
                    .unbounded_send(monitor::Message::Off)
                    .expect("BUG: send failed");

                return Err(e)?;
            }
            Ok(a) => a,
        };

        // spawn worker tasks for hash chain and start mining
        let hash_chain = Arc::new(hash_chain);
        hash_chain
            .clone()
            .start(
                self.work_generator.clone(),
                self.solution_sender.clone(),
                work_registry,
            )
            .await;

        // remember we started
        inner.hash_chain.replace(hash_chain);

        Ok(())
    }

    /// TODO: this function is private and should be called only from `RunningChain`
    async fn stop_chain(&self, its_ok_if_its_missing: bool) {
        // lock inner to guarantee atomicity of hashchain stop
        let mut inner = self.inner.lock().await;

        // TODO: maybe we should throw an error instead
        let hash_chain = inner.hash_chain.take();
        if hash_chain.is_none() && its_ok_if_its_missing {
            return;
        }
        let hash_chain = hash_chain.expect("BUG: hashchain is missing");

        // stop everything
        hash_chain.halt_sender.clone().send_halt().await;

        // tell monitor we are done
        self.monitor_tx
            .unbounded_send(monitor::Message::Off)
            .expect("BUG: send failed");
    }

    async fn termination_handler(self: Arc<Self>) {
        self.stop_chain(true).await;
    }
}

#[async_trait]
impl node::WorkSolver for Manager {
    fn get_id(&self) -> Option<usize> {
        Some(self.hashboard_idx)
    }

    async fn get_nominal_hashrate(&self) -> Option<ii_bitcoin::HashesUnit> {
        let inner = self.inner.lock().await;
        match inner.hash_chain.as_ref() {
            Some(hash_chain) => {
                let freq_sum = hash_chain.frequency.lock().await.total();
                Some(((freq_sum as u128) * (bm1387::NUM_CORES_ON_CHIP as u128)).into())
            }
            None => None,
        }
    }
}

impl fmt::Debug for Manager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash Chain {}", self.hashboard_idx)
    }
}

impl fmt::Display for Manager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash Chain {}", self.hashboard_idx)
    }
}

#[derive(Debug, WorkSolverNode)]
pub struct Backend {
    #[member_work_solver_stats]
    work_solver_stats: stats::BasicWorkSolver,
}

impl Backend {
    pub fn new() -> Self {
        Self {
            work_solver_stats: Default::default(),
        }
    }

    /// Enumerate present hashboards by querying the plug pin
    pub fn detect_hashboards(gpio_mgr: &gpio::ControlPinManager) -> error::Result<Vec<usize>> {
        let mut detected = vec![];
        // TODO: configure this range somewhere
        for hashboard_idx in 1..=8 {
            let plug_pin = hashchain::PlugPin::open(gpio_mgr, hashboard_idx)?;
            if plug_pin.hashboard_present()? {
                detected.push(hashboard_idx);
            }
        }
        Ok(detected)
    }

    /// Miner termination handler called when app is shutdown.
    /// Just propagate the shutdown to all hashchain managers
    async fn termination_handler(halt_sender: Arc<halt::Sender>) {
        halt_sender.send_halt().await;
    }

    /// Start miner
    /// TODO: maybe think about having a `Result` error value here?
    async fn start_miner(
        gpio_mgr: &gpio::ControlPinManager,
        enabled_chains: Vec<usize>,
        work_hub: work::SolverBuilder<Backend>,
        backend_config: config::Backend,
        app_halt_receiver: halt::Receiver,
        app_halt_sender: Arc<halt::Sender>,
    ) -> (Vec<Arc<Manager>>, Arc<monitor::Monitor>) {
        // Create hooks
        let hooks = match backend_config.hooks.as_ref() {
            Some(hooks) => hooks.clone(),
            None => Arc::new(hooks::NoHooks),
        };

        // Create new termination context and link it to the main (app) termination context
        let (halt_sender, halt_receiver) = halt::make_pair(HALT_TIMEOUT);
        app_halt_receiver
            .register_client("miner termination".into())
            .await
            .spawn_halt_handler(Self::termination_handler(halt_sender.clone()));
        hooks
            .halt_created(
                halt_sender.clone(),
                halt_receiver.clone(),
                app_halt_sender.clone(),
            )
            .await;

        // Start monitor in main (app) termination context
        // Let it shutdown the main context as well
        let monitor_config = backend_config.resolve_monitor_config();
        info!("Resolved monitor backend_config: {:?}", monitor_config);
        let monitor = monitor::Monitor::new_and_start(
            monitor_config,
            app_halt_sender.clone(),
            app_halt_receiver.clone(),
        )
        .await;
        hooks.monitor_started(monitor.clone()).await;

        let voltage_ctrl_backend = Arc::new(power::I2cBackend::new(0));
        let mut managers = Vec::new();
        info!(
            "Initializing miner, enabled_chains={:?}, midstate_count={}",
            enabled_chains,
            backend_config.midstate_count(),
        );
        // build all hash chain managers and register ourselves with frontend
        for hashboard_idx in enabled_chains {
            // register monitor for this haschain
            let monitor_tx = monitor.register_hashchain(hashboard_idx).await;
            // make pins
            let chain_config = backend_config.resolve_chain_config(hashboard_idx);

            let status_receiver = monitor.status_receiver.clone();

            // build hashchain_node for statistics and static parameters
            let manager = work_hub
                .create_work_solver(|work_generator, solution_sender| {
                    Manager {
                        // TODO: create a new substructure of the miner that will hold all gpio and
                        // "physical-insertion" detection data. This structure will be persistent in
                        // between restarts and will enable early notification that there is no hashboard
                        // inserted (instead find out at mining-time).
                        reset_pin: hashchain::ResetPin::open(&gpio_mgr, hashboard_idx)
                            .expect("failed to make pin"),
                        plug_pin: hashchain::PlugPin::open(&gpio_mgr, hashboard_idx)
                            .expect("failed to make pin"),
                        voltage_ctrl_backend: voltage_ctrl_backend.clone(),
                        hashboard_idx,
                        midstate_count: chain_config.midstate_count,
                        work_solver_stats: Default::default(),
                        solution_sender,
                        work_generator,
                        monitor_tx,
                        status_receiver,
                        owned_by: StdMutex::new(None),
                        inner: Mutex::new(ManagerInner {
                            hash_chain: None,
                            start_count: 0,
                        }),
                        chain_config,
                    }
                })
                .await;
            managers.push(manager);
        }

        // start everything
        for manager in managers.iter() {
            let halt_receiver = halt_receiver.clone();
            let manager = manager.clone();

            let initial_frequency = manager.chain_config.frequency.clone();
            let initial_voltage = manager.chain_config.voltage;
            let hooks = hooks.clone();

            // Register handler to stop hashchain when miner is stopped
            halt_receiver
                .register_client("hashchain".into())
                .await
                .spawn_halt_handler(Manager::termination_handler(manager.clone()));

            // Suppress haschain start if chain is either not enabled or haschain hook doesn't
            // want us to start it (default `NoHooks` has all chains enabled).
            if hooks.can_start_chain(manager.clone()).await {
                tokio::spawn(async move {
                    manager
                        .acquire("main")
                        .await
                        .expect("BUG: failed to acquire hashchain")
                        .expect_stopped()
                        .start(
                            &initial_frequency,
                            initial_voltage,
                            config::DEFAULT_ASIC_DIFFICULTY,
                        )
                        .await
                        .expect("BUG: failed to start hashchain");
                });
            }
        }
        hooks.miner_started().await;
        (managers, monitor)
    }
}

#[async_trait]
impl hal::Backend for Backend {
    type Type = Self;
    type Config = config::Backend;

    const DEFAULT_HASHRATE_INTERVAL: Duration = config::DEFAULT_HASHRATE_INTERVAL;
    const JOB_TIMEOUT: Duration = config::JOB_TIMEOUT;

    fn create(_backend_config: &mut config::Backend) -> hal::WorkNode<Self> {
        node::WorkSolverType::WorkHub(Box::new(Self::new))
    }

    async fn init_work_hub(
        mut backend_config: config::Backend,
        work_hub: work::SolverBuilder<Self>,
    ) -> bosminer::Result<hal::FrontendConfig> {
        let hooks = backend_config.hooks.clone();
        // Prepare data for pool configuration after successful start of backend
        let client_manager = backend_config
            .client_manager
            .take()
            .expect("BUG: missing client manager");
        let group_configs = backend_config.groups.take();
        let backend_info = backend_config.info();

        let backend = work_hub.to_node().clone();
        let gpio_mgr = gpio::ControlPinManager::new();
        let (app_halt_sender, app_halt_receiver) = halt::make_pair(HALT_TIMEOUT);
        let (managers, monitor) = Self::start_miner(
            &gpio_mgr,
            Self::detect_hashboards(&gpio_mgr).expect("failed detecting hashboards"),
            work_hub,
            backend_config,
            app_halt_receiver,
            app_halt_sender.clone(),
        )
        .await;

        // On miner exit, halt the whole program
        app_halt_sender
            .add_exit_hook(async {
                println!("Exiting.");
                std::process::exit(0);
            })
            .await;
        // Hook `Ctrl-C`, `SIGTERM` and other termination methods
        app_halt_sender.hook_termination_signals();

        // Load initial pool configuration
        client_manager
            .load_config(
                group_configs,
                backend_info.as_ref(),
                config::DEFAULT_POOL_ENABLED,
            )
            .await?;
        if let Some(hooks) = hooks.as_ref() {
            // Pass the client manager to hook for further processing
            hooks.clients_loaded(client_manager).await;
        }

        let mut cgminer_custom_commands =
            cgminer::create_custom_commands(backend, managers, monitor);
        if let Some(hooks) = hooks.as_ref() {
            // Let hooks have a go at the commands
            hooks
                .clone()
                .cgminer_custom_commands_created(&mut cgminer_custom_commands)
                .await;
        }

        Ok(hal::FrontendConfig {
            cgminer_custom_commands: Some(cgminer_custom_commands),
        })
    }

    async fn init_work_solver(
        _backend_config: config::Backend,
        _work_solver: Arc<Self>,
    ) -> bosminer::Result<hal::FrontendConfig> {
        panic!("BUG: called `init_work_solver`");
    }
}

#[async_trait]
impl node::WorkSolver for Backend {
    async fn get_nominal_hashrate(&self) -> Option<ii_bitcoin::HashesUnit> {
        None
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bitmain Antminer S9")
    }
}
