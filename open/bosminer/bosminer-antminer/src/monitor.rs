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

//! This module is responsible for collecting temperatures from hashchains and driving
//! the fans.

use ii_logging::macros::*;

use crate::fan;
use crate::halt;

use ii_sensors::{self as sensor, Measurement};

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::channel::mpsc;
use futures::lock::{Mutex, MutexGuard};
use futures::stream::StreamExt;
use ii_async_compat::futures;
use ii_async_compat::tokio;
use tokio::sync::watch;
use tokio::time::delay_for;

/// If miner start takes longer than this, mark it as `Broken`
const START_TIMEOUT: Duration = Duration::from_secs(180);
/// If miner doesn't send temperature update within this time, mark it as dead.
/// This timeout doubles as hashchain watchdog timeout.
const RUN_UPDATE_TIMEOUT: Duration = Duration::from_secs(20);
/// How often to update temperature - should be strictly less than `RUN_UPDATE_TIMEOUT`
pub const TEMP_UPDATE_INTERVAL: Duration = Duration::from_secs(5);
/// How often check timeouts and adjust PID
const TICK_LENGTH: Duration = Duration::from_secs(5);
/// How long does it take until miner warm up? We won't let it tu turn fans off until then...
const WARM_UP_PERIOD: Duration = Duration::from_secs(90);

/// A message from hashchain
///
/// Here are some rules that HashChains registered with monitors have to obey:
///
/// - state change must be strictly `[Off -> On -> Running*]*`
/// - duration between `On` and first `Running` must be less than START_TIMEOUT
/// - duration between `Running` measurement and the next one must be less than
///   RUN_UPDATE_INTERVAL (ideally set periodic update to half of this interval)
#[derive(Debug, Clone)]
pub enum Message {
    On,
    Running(sensor::Temperature),
    Off,
}

/// Interpreted hashchain temperature
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChainTemperature {
    /// Temperature unknown... in a good way (hashchain initializing, etc.)
    Unknown,
    /// Temperature unknown... in a bad way (miner caught fire, etc.)
    Failed,
    /// Temperature was measured
    Ok(f32),
}

impl ChainTemperature {
    /// Convert temperature to monitor interpretation.
    /// Specific to S9, because it fakes chip temperature.
    ///
    /// TODO: Maybe figure out a strage for disabling remote sensors that are failing. Sometimes
    /// remote sensors fail while mining and instead of signalizing error they return non-sensical
    /// numbers.
    /// TODO: Is returning "Unknown" when sensor fails OK?
    fn from_s9_sensor(temp: sensor::Temperature) -> Self {
        match temp.remote {
            // remote is chip temperature
            Measurement::Ok(t_remote) => match temp.local {
                Measurement::Ok(t_local) => Self::Ok(t_remote.max(t_local)),
                _ => Self::Ok(t_remote),
            },
            _ => {
                // fake chip temperature from local (PCB) temperature
                match temp.local {
                    Measurement::Ok(t_local) => Self::Ok(t_local + 15.0),
                    _ => Self::Unknown,
                }
            }
        }
    }
}

impl fmt::Display for ChainTemperature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChainTemperature::Unknown => write!(f, "???"),
            ChainTemperature::Failed => write!(f, "Fail"),
            ChainTemperature::Ok(t) => write!(f, "{:.0}°C", t),
        }
    }
}

/// State of hashchain as seen from Monitor point of view
/// The `Instant` timestamps are when that event happen (only states that operate with
/// timeouts use it).
#[derive(Debug, Clone, PartialEq)]
enum ChainState {
    On(Instant),
    Running {
        started: Instant,
        last_heartbeat: Instant,
        temperature: sensor::Temperature,
    },
    Off,
    Broken(&'static str),
}

impl ChainState {
    /// Go into invalid state (no way out)
    fn bad_transition(&mut self) {
        *self = ChainState::Broken("bad state transition");
    }

    /// React on an incoming message by changing modifying state. All messages
    /// have follow pattern `[Off -> On -> Running*]*`
    ///
    /// `now` is timestamp of `message` reception (passed explicitly as argument
    /// to facilitate testing).
    fn transition(&mut self, now: Instant, message: Message) {
        match message {
            Message::On => match *self {
                ChainState::Off => *self = ChainState::On(now),
                _ => self.bad_transition(),
            },
            Message::Running(temperature) => match *self {
                ChainState::Running { started, .. } | ChainState::On(started) => {
                    *self = ChainState::Running {
                        started,
                        last_heartbeat: now,
                        temperature,
                    }
                }
                _ => self.bad_transition(),
            },
            Message::Off => match *self {
                ChainState::On(_) | ChainState::Running { .. } => *self = ChainState::Off,
                _ => self.bad_transition(),
            },
        }
    }

    /// Do a timer tick: check all timeouts and do appropriate state transitions.
    /// If miner is starting, check it starts in `START_TIMEOUT`, if its running, check
    /// it's sending "heartbeats" often enought.
    fn tick(&mut self, now: Instant) {
        match *self {
            ChainState::On(started) => {
                if now.duration_since(started) >= START_TIMEOUT {
                    *self = ChainState::Broken("took too long to start");
                }
            }
            ChainState::Running { last_heartbeat, .. } => {
                if now.duration_since(last_heartbeat) >= RUN_UPDATE_TIMEOUT {
                    *self = ChainState::Broken("failed to set update in time");
                }
            }
            _ => {}
        }
    }

    /// Return hashchain temperature as seen from our point of view. For example,
    /// `Broken` miner doesn't have a valid temperature reading even though it sent
    /// some numbers a while ago.
    fn get_temperature(&self) -> ChainTemperature {
        match self {
            ChainState::On(_) => ChainTemperature::Unknown,
            ChainState::Off => ChainTemperature::Unknown,
            ChainState::Broken(_) => ChainTemperature::Failed,
            ChainState::Running { temperature, .. } => {
                ChainTemperature::from_s9_sensor(temperature.clone())
            }
        }
    }

    /// Is hashchain warming up?
    fn is_warming_up(&self, now: Instant) -> bool {
        match self {
            // chain state stays in "warming up" state until it sends heartbeat
            ChainState::On(_) => true,
            ChainState::Running { started, .. } => now.duration_since(*started) <= WARM_UP_PERIOD,
            _ => false,
        }
    }
}

impl fmt::Display for ChainState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChainState::On { .. } => write!(f, "Starting"),
            ChainState::Off => write!(f, "Off"),
            ChainState::Running { temperature, .. } => write!(
                f,
                "{},{}",
                temperature.local.to_string(),
                temperature.remote.to_string()
            ),
            ChainState::Broken { .. } => write!(f, "Broken"),
        }
    }
}

/// Represent hashchains as registered within Monitor
struct Chain {
    state: ChainState,
    hashboard_idx: usize,
}

impl Chain {
    fn new(hashboard_idx: usize) -> Self {
        Self {
            state: ChainState::Off,
            hashboard_idx,
        }
    }
}

/// What method of controlling fans is configured
#[derive(Debug, Clone)]
pub enum FanControlMode {
    FixedSpeed(fan::Speed),
    TargetTemperature(f32),
}

/// Fan configuration
#[derive(Debug, Clone)]
pub struct FanControlConfig {
    pub mode: FanControlMode,
    /// Minimal number of fans - miner will refuse to work until at least
    /// this number of fans is spinning.
    pub min_fans: usize,
}

/// Temperature limit configuration
#[derive(Debug, Clone)]
pub struct TempControlConfig {
    pub dangerous_temp: f32,
    pub hot_temp: f32,
}

/// Overall configuration
/// "Disabled" is represented as `None`
#[derive(Debug, Clone)]
pub struct Config {
    pub fan_config: Option<FanControlConfig>,
    pub temp_config: Option<TempControlConfig>,
    /// If true, then do not let fans bellow predefined limit while miner is warming up.
    /// TODO: this is not particularly nice, it should be done per-chain and run-time.
    pub fans_on_while_warming_up: bool,
}

#[derive(Debug, Clone)]
pub struct ControlDecisionExplained {
    pub decision: ControlDecision,
    pub reason: String,
}

/// Output of the decision process
#[derive(Debug, Clone, PartialEq)]
pub enum ControlDecision {
    /// Fail state - shutdown miner
    Shutdown,
    /// Pass these parameters to PID and let it calculate fan speed
    UsePid { target_temp: f32, input_temp: f32 },
    /// Use fixed speed
    UseFixedSpeed(fan::Speed),
    /// Do nothing (only valid when fan control is disabled)
    Nothing,
}

impl ControlDecision {
    /// Decision rules if both fan control and temp control are enabled
    fn decide_fan_control(
        fan_config: &FanControlConfig,
        temp_config: &TempControlConfig,
        temp: ChainTemperature,
    ) -> ControlDecisionExplained {
        if temp == ChainTemperature::Unknown {
            return ControlDecisionExplained {
                decision: Self::UseFixedSpeed(fan::Speed::FULL_SPEED),
                reason: "Fans full speed: unknown temperature".into(),
            };
        }
        match &fan_config.mode {
            FanControlMode::FixedSpeed(pwm) => {
                return ControlDecisionExplained {
                    decision: Self::UseFixedSpeed(*pwm),
                    reason: format!("User defined fan {}", *pwm),
                };
            }
            FanControlMode::TargetTemperature(target_temp) => match temp {
                ChainTemperature::Failed | ChainTemperature::Unknown => {
                    panic!("BUG: should've been caught earlier at the top of `decide()` function")
                }
                ChainTemperature::Ok(input_temp) => {
                    if input_temp >= temp_config.hot_temp {
                        return ControlDecisionExplained {
                            decision: Self::UseFixedSpeed(fan::Speed::FULL_SPEED),
                            reason: format!("Fans full speed: temperature {} above HOT", temp),
                        };
                    }
                    return ControlDecisionExplained {
                        decision: Self::UsePid {
                            target_temp: *target_temp,
                            input_temp,
                        },
                        reason: format!(
                            "Automatic fan control: input {} target {:.0}°C",
                            temp, target_temp
                        ),
                    };
                }
            },
        }
    }

    /// Decision rules if fan control is enabled and temp control disabled
    fn decide_fan_control_notemp(fan_config: &FanControlConfig) -> ControlDecisionExplained {
        match fan_config.mode {
            FanControlMode::FixedSpeed(pwm) => {
                return ControlDecisionExplained {
                    decision: Self::UseFixedSpeed(pwm),
                    reason: format!("Fans to {} (user defined)", pwm),
                };
            }
            FanControlMode::TargetTemperature(_) => {
                // I don't know how to avoid this variant using type system alone
                // Let's make it non-fatal
                return ControlDecisionExplained {
                    decision: Self::UseFixedSpeed(fan::Speed::FULL_SPEED),
                    reason: "wrong configuration - temp control off".into(),
                };
            }
        }
    }

    /// Decide what to do depending on temperature/fan feedback.
    /// This function has been factored out of the main control code to facilitate testing.
    fn decide(
        config: &Config,
        num_fans_running: usize,
        temp: ChainTemperature,
    ) -> ControlDecisionExplained {
        // This section is labeled `TEMP_DANGER` in the diagram
        // Check for dangerous temperature or dead sensors
        if let Some(temp_config) = config.temp_config.as_ref() {
            match temp {
                ChainTemperature::Failed => {
                    return ControlDecisionExplained {
                        decision: Self::Shutdown,
                        reason: "Shutdown: temperature readout FAILED".into(),
                    };
                }
                ChainTemperature::Ok(input_temp) => {
                    if input_temp >= temp_config.dangerous_temp {
                        return ControlDecisionExplained {
                            decision: Self::Shutdown,
                            reason: format!("Shutdown: temperature {} above DANGEROUS", temp),
                        };
                    }
                }
                ChainTemperature::Unknown => {}
            }
        }
        // Check the health of fans and decide their speed
        if let Some(fan_config) = config.fan_config.as_ref() {
            let decision_explained = if let Some(temp_config) = config.temp_config.as_ref() {
                Self::decide_fan_control(fan_config, temp_config, temp)
            } else {
                Self::decide_fan_control_notemp(fan_config)
            };
            // This section is labeled `FAN_DANGER` in the diagram
            //
            // Check `min_fans` are spinning _unless_ we have been explicitly configured to
            // turn them off.
            //
            // XXX: There's a problem however: if we are configured for stopped fans and then
            // the configuration changes at runtime to non-stopped fans, the delay of fans
            // taking some time to spin up will cause this check to fire off!
            if decision_explained.decision != Self::UseFixedSpeed(fan::Speed::STOPPED) {
                if num_fans_running < fan_config.min_fans {
                    return ControlDecisionExplained {
                        decision: Self::Shutdown,
                        reason: format!(
                            "Shutdown: not enough fans ({} < {})",
                            num_fans_running, fan_config.min_fans
                        ),
                    };
                }
            }
            decision_explained
        } else {
            // This is only valid if `FanControl` is turned off
            ControlDecisionExplained {
                decision: Self::Nothing,
                reason: "control disabled".into(),
            }
        }
    }
}

/// This structure abstracts the process of "making one aggregate temperature out of
/// all hashchain temperatures".
/// The resulting temperature is used as an input variable for PID control.
#[derive(Debug, Clone)]
pub struct TemperatureAccumulator {
    pub chain_temperatures: Vec<ChainTemperature>,
}

impl TemperatureAccumulator {
    fn new() -> Self {
        Self {
            chain_temperatures: vec![],
        }
    }

    fn add_chain_temp(&mut self, chain_temp: ChainTemperature) {
        self.chain_temperatures.push(chain_temp);
    }

    /// Function to calculate aggregated temperature.
    /// This one calculates maximum temperatures over all temperatures measured while
    /// prefering failures to measurement.
    fn calc_result(&self) -> ChainTemperature {
        let mut temps = vec![];
        for &temp in self.chain_temperatures.iter() {
            match temp {
                // Failure thrumps everything
                ChainTemperature::Failed => return temp,
                // Unknown temperature doesn't add any information
                ChainTemperature::Unknown => (),
                // Collect measurements
                ChainTemperature::Ok(t) => temps.push(t),
            }
        }
        // If we collected any temperatures, take maximum of them, otherwise return unknown
        if temps.len() > 0 {
            ChainTemperature::Ok(temps.drain(..).fold(0.0, |a, b| a.max(b)))
        } else {
            ChainTemperature::Unknown
        }
    }
}

/// Status of `Monitor` for others to observe
#[derive(Debug, Clone)]
pub struct Status {
    pub config: Config,
    pub fan_feedback: fan::Feedback,
    pub fan_speed: Option<fan::Speed>,
    pub input_temperature: ChainTemperature,
    pub temperature_accumulator: TemperatureAccumulator,
    pub decision_explained: ControlDecisionExplained,
}

/// Monitor - it holds states of all Chains and everything related to fan control
pub struct MonitorInner {
    /// Each chain is registered here
    chains: Vec<Arc<Mutex<Chain>>>,
    /// temp/fan control configuration
    config: Config,
    /// Fan controller - can set RPM or read feedback
    fan_control: fan::Control,
    /// Last fan speed that was set
    current_fan_speed: Option<fan::Speed>,
    /// PID that controls fan with hashchain temperature as input
    pid: fan::pid::TempControl,
    /// Flag whether miner is in failure state - temperature critical, hashboards not responding,
    /// fans gone missing...
    failure_state: bool,
}

/// Wrapper around `MonitorInner` with immutable fields
pub struct Monitor {
    /// Broadcast channel to send/receive monitor status
    status_sender: watch::Sender<Option<Status>>,
    pub status_receiver: watch::Receiver<Option<Status>>,

    /// Context to shutdown when miner enters critical state
    miner_shutdown: Arc<halt::Sender>,

    /// Inner context
    inner: Mutex<MonitorInner>,
}

impl Monitor {
    /// Construct a new monitor and start it
    ///
    /// * `miner_shutdown` - halt sender to shutdown the whole miner in case of a failure
    /// * `halt_receiver` - termination context in which to start the monitor
    pub async fn new_and_start(
        config: Config,
        miner_shutdown: Arc<halt::Sender>,
        halt_receiver: halt::Receiver,
    ) -> Arc<Self> {
        let (status_sender, status_receiver) = watch::channel(None);

        let inner = MonitorInner {
            chains: Vec::new(),
            config,
            fan_control: fan::Control::new().expect("failed initializing fan controller"),
            pid: fan::pid::TempControl::new(),
            failure_state: false,
            current_fan_speed: None,
        };

        let monitor = Arc::new(Monitor {
            miner_shutdown,
            status_sender,
            status_receiver,
            inner: Mutex::new(inner),
        });

        halt_receiver
            .register_client("monitor termination".into())
            .await
            .spawn_halt_handler(Self::termination_handler(monitor.clone()));

        halt_receiver
            .register_client("monitor".into())
            .await
            .spawn(Self::tick_task(monitor.clone()));

        monitor
    }

    /// Handler that is run when monitor is signalized with shutdown.
    /// Just stops the fans (depending on whether it's in failure state).
    async fn termination_handler(self: Arc<Self>) {
        let mut inner = self.inner.lock().await;
        // Decide whether to leave fans on (depending on whether we are in failure state or not)
        if inner.failure_state {
            self.set_fan_speed(&mut inner, fan::Speed::FULL_SPEED);
        } else {
            self.set_fan_speed(&mut inner, fan::Speed::STOPPED);
        }
    }

    /// Shutdown miner
    async fn shutdown(&self, mut inner: MutexGuard<'_, MonitorInner>, reason: String) {
        error!("Monitor task declared miner shutdown: {}", reason);
        inner.failure_state = true;
        // Shutdown handler locks `inner`, so drop the guard here to prevent deadlock
        drop(inner);
        self.miner_shutdown.clone().send_halt().await;
    }

    /// Set fan speed
    fn set_fan_speed(&self, inner: &mut MonitorInner, fan_speed: fan::Speed) {
        trace!("Monitor: setting fan to {:?}", fan_speed);
        inner.fan_control.set_speed(fan_speed);
        inner.current_fan_speed = Some(fan_speed);
    }

    /// One tick of temperature/fan controller
    ///
    /// TODO: Run this tick every time new temperature is submitted to lower temp controller
    ///   latency.
    async fn do_tick(&self) {
        // decide hashchain state and collect temperatures
        let mut inner = self.inner.lock().await;
        let mut temperature_accumulator = TemperatureAccumulator::new();
        let mut miner_warming_up = false;
        let mut chain_info_status = vec![];
        for chain in inner.chains.iter() {
            let mut chain = chain.lock().await;
            chain.state.tick(Instant::now());

            if let ChainState::Broken(reason) = chain.state {
                // TODO: here comes "Shutdown"
                let reason = format!("Chain {} is broken: {}", chain.hashboard_idx, reason);
                // drop `chain` here to drop iterator which holds immutable reference
                // to `monitor`
                drop(chain);
                self.shutdown(inner, reason).await;
                return;
            }
            trace!("Monitor: chain {}: {:?}", chain.hashboard_idx, chain.state);
            chain_info_status.push(chain.state.to_string());
            temperature_accumulator.add_chain_temp(chain.state.get_temperature());
            miner_warming_up |= chain.state.is_warming_up(Instant::now());
        }
        let input_temperature = temperature_accumulator.calc_result();

        // Read fans
        let fan_feedback = inner.fan_control.read_feedback();
        let num_fans_running = fan_feedback.num_fans_running();
        trace!(
            "Monitor: fan={:?} num_fans={} acc.temp.={:?}",
            fan_feedback,
            num_fans_running,
            input_temperature,
        );
        // all right, temperature has been aggregated, decide what to do
        let decision_explained =
            ControlDecision::decide(&inner.config, num_fans_running, input_temperature);
        trace!("Monitor: {:?}", decision_explained);
        let status_line = format!(
            "{} | {} | {}",
            decision_explained.reason,
            chain_info_status.join(" "),
            fan_feedback.to_string(),
        );
        match decision_explained.decision {
            ControlDecision::Shutdown => {
                info!("Monitor: {}", status_line);
                self.shutdown(inner, decision_explained.reason.into()).await;
                return;
            }
            ControlDecision::UseFixedSpeed(fan_speed) => {
                info!("Monitor: {} fan_{}", status_line, fan_speed);
                self.set_fan_speed(&mut inner, fan_speed);
            }
            ControlDecision::UsePid {
                target_temp,
                input_temp,
            } => {
                if inner.config.fans_on_while_warming_up && miner_warming_up {
                    inner.pid.set_warm_up_limits();
                } else {
                    inner.pid.set_normal_limits();
                }
                inner.pid.set_target(target_temp.into());
                let speed = inner.pid.update(input_temp.into());
                self.set_fan_speed(&mut inner, speed);
                info!("Monitor: {} fan_{}", status_line, speed);
            }
            ControlDecision::Nothing => {}
        }

        // Broadcast `Status`
        let monitor_status = Status {
            fan_feedback,
            fan_speed: inner.current_fan_speed,
            input_temperature,
            temperature_accumulator,
            decision_explained,
            config: inner.config.clone(),
        };
        self.status_sender
            .broadcast(Some(monitor_status))
            .expect("broadcast failed");
    }

    /// Task performing temp control
    async fn tick_task(self: Arc<Self>) {
        loop {
            self.do_tick().await;
            // TODO: find some of kind "run every x secs" function
            delay_for(TICK_LENGTH).await;
        }
    }

    /// Per-chain task that collects hashchain status update messages
    async fn recv_task(chain: Arc<Mutex<Chain>>, mut rx: mpsc::UnboundedReceiver<Message>) {
        while let Some(message) = rx.next().await {
            let mut chain = chain.lock().await;
            chain.state.transition(Instant::now(), message);
        }
    }

    /// Registers hashchain within monitor
    /// The `hashboard_idx` parameter is for debugging purposes
    pub async fn register_hashchain(&self, hashboard_idx: usize) -> mpsc::UnboundedSender<Message> {
        let (tx, rx) = mpsc::unbounded();
        let chain = Arc::new(Mutex::new(Chain::new(hashboard_idx)));
        {
            let mut inner = self.inner.lock().await;
            inner.chains.push(chain.clone());
            tokio::spawn(Self::recv_task(chain, rx));
        }
        tx
    }

    pub async fn with_configuration<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Config) -> R,
    {
        let mut inner = self.inner.lock().await;
        f(&mut inner.config)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use approx::assert_relative_eq;

    macro_rules! assert_variant {
        ($value:expr, $pattern:pat) => {{
            let value = &$value;
            if let $pattern = value {
            } else {
                panic!(
                    r#"assertion failed (value doesn't match pattern):
                        value: `{:?}`,
                        pattern: `{}`"#,
                    value,
                    stringify!($pattern)
                )
            }
        }}; // TODO: Additional patterns for trailing args, like assert and assert_eq
    }

    /// Test that faking S9 chip temperature from board temperature works
    #[test]
    fn test_monitor_s9_chip_temp() {
        let temp = sensor::Temperature {
            local: sensor::Measurement::Ok(10.0),
            remote: sensor::Measurement::Ok(22.0),
        };
        match ChainTemperature::from_s9_sensor(temp) {
            ChainTemperature::Ok(t) => assert_relative_eq!(t, 22.0),
            _ => panic!("missing temperature"),
        };
        let temp = sensor::Temperature {
            local: sensor::Measurement::Ok(10.0),
            remote: sensor::Measurement::OpenCircuit,
        };
        match ChainTemperature::from_s9_sensor(temp) {
            ChainTemperature::Ok(t) => assert_relative_eq!(t, 25.0),
            _ => panic!("missing temperature"),
        };
        let temp = sensor::Temperature {
            local: sensor::Measurement::InvalidReading,
            remote: sensor::Measurement::OpenCircuit,
        };
        assert_eq!(
            ChainTemperature::from_s9_sensor(temp),
            ChainTemperature::Unknown
        );
    }

    fn send(mut state: ChainState, when: Instant, message: Message) -> ChainState {
        state.transition(when, message);
        state
    }

    /// Test that miner transitions states as expected
    #[test]
    fn test_monitor_state_transition() {
        let temp = sensor::Temperature {
            local: sensor::Measurement::Ok(10.0),
            remote: sensor::Measurement::Ok(22.0),
        };
        let now = Instant::now();
        let later = now + Duration::from_secs(1);
        let running_state = ChainState::Running {
            started: now,
            last_heartbeat: now,
            temperature: temp.clone(),
        };

        //assert_eq!(send(ChainState::Running(now, temp), later, Message::Off), ChainState::Off);
        assert_variant!(send(ChainState::Off, later, Message::On), ChainState::On(_));
        assert_variant!(
            send(ChainState::Off, later, Message::Running(temp.clone())),
            ChainState::Broken(_)
        );
        assert_variant!(
            send(ChainState::Off, later, Message::Off),
            ChainState::Broken(_)
        );

        assert_variant!(
            send(ChainState::On(now), later, Message::On),
            ChainState::Broken(_)
        );
        assert_variant!(
            send(ChainState::On(now), later, Message::Running(temp.clone())),
            ChainState::Running{ .. }
        );
        assert_variant!(
            send(ChainState::On(now), later, Message::Off),
            ChainState::Off
        );

        assert_variant!(
            send(running_state.clone(), later, Message::On),
            ChainState::Broken(_)
        );
        assert_variant!(
            send(
                running_state.clone(),
                later,
                Message::Running(temp.clone())
            ),
            ChainState::Running { .. }
        );
        assert_variant!(
            send(running_state.clone(), later, Message::Off),
            ChainState::Off
        );
    }

    /// Test "warm up" period
    #[test]
    fn test_monitor_warm_up() {
        let temp = sensor::Temperature {
            local: sensor::Measurement::Ok(10.0),
            remote: sensor::Measurement::Ok(22.0),
        };
        let now = Instant::now();
        let later = now + Duration::from_secs(20);
        let warmed_time = now + Duration::from_secs(200);
        let running_state = ChainState::Running {
            started: now,
            last_heartbeat: now,
            temperature: temp.clone(),
        };

        assert_eq!(ChainState::Off.is_warming_up(now), false);
        assert_eq!(ChainState::On(now).is_warming_up(now), true);
        assert_eq!(ChainState::On(now).is_warming_up(warmed_time), true);
        assert_eq!(running_state.clone().is_warming_up(now), true);
        assert_eq!(running_state.clone().is_warming_up(later), true);
        assert_eq!(running_state.clone().is_warming_up(warmed_time), false);
    }

    fn tick(mut state: ChainState, later: Instant) -> ChainState {
        state.tick(later);
        state
    }

    /// Test timeouts
    #[test]
    fn test_monitor_timeouts() {
        let temp = sensor::Temperature {
            local: sensor::Measurement::Ok(10.0),
            remote: sensor::Measurement::Ok(22.0),
        };
        let now = Instant::now();
        let long = now + Duration::from_secs(10_000);
        let short = now + Duration::from_secs(2);
        let running_state = ChainState::Running {
            started: now,
            last_heartbeat: now,
            temperature: temp.clone(),
        };

        // test that chains break when no-one updates them for long (unless they are turned off)
        assert_variant!(tick(ChainState::Off, long), ChainState::Off);
        assert_variant!(tick(ChainState::On(now), long), ChainState::Broken(_));
        assert_variant!(tick(running_state.clone(), long), ChainState::Broken(_));

        // passing of short time is OK
        assert_variant!(tick(ChainState::Off, short), ChainState::Off);
        assert_variant!(tick(ChainState::On(now), short), ChainState::On(_));
        assert_variant!(
            tick(running_state.clone(), short),
            ChainState::Running{..}
        );

        // different states have different update timeouts
        assert_variant!(
            tick(ChainState::On(now), now + Duration::from_secs(20)),
            ChainState::On(_)
        );
        assert_variant!(
            tick(running_state.clone(), now + Duration::from_secs(20)),
            ChainState::Broken(_)
        );
    }

    fn test_acc(temp1: ChainTemperature, temp2: ChainTemperature) -> ChainTemperature {
        let mut tacc = TemperatureAccumulator::new();
        tacc.add_chain_temp(temp1);
        tacc.add_chain_temp(temp2);
        tacc.calc_result()
    }

    /// Test temperature accumulator
    #[test]
    fn test_monitor_temp_acc() {
        assert_eq!(
            test_acc(ChainTemperature::Unknown, ChainTemperature::Unknown),
            ChainTemperature::Unknown
        );
        assert_eq!(
            test_acc(ChainTemperature::Failed, ChainTemperature::Unknown),
            ChainTemperature::Failed
        );
        assert_eq!(
            test_acc(ChainTemperature::Ok(10.0), ChainTemperature::Unknown),
            ChainTemperature::Ok(10.0)
        );
        assert_eq!(
            test_acc(ChainTemperature::Unknown, ChainTemperature::Failed),
            ChainTemperature::Failed
        );
        assert_eq!(
            test_acc(ChainTemperature::Failed, ChainTemperature::Failed),
            ChainTemperature::Failed
        );
        assert_eq!(
            test_acc(ChainTemperature::Ok(10.0), ChainTemperature::Failed),
            ChainTemperature::Failed
        );
        assert_eq!(
            test_acc(ChainTemperature::Unknown, ChainTemperature::Ok(20.0)),
            ChainTemperature::Ok(20.0)
        );
        assert_eq!(
            test_acc(ChainTemperature::Failed, ChainTemperature::Ok(20.0)),
            ChainTemperature::Failed
        );
        assert_eq!(
            test_acc(ChainTemperature::Ok(10.0), ChainTemperature::Ok(20.0)),
            ChainTemperature::Ok(20.0)
        );
        assert_eq!(
            test_acc(ChainTemperature::Ok(10.0), ChainTemperature::Ok(5.0)),
            ChainTemperature::Ok(10.0)
        );
    }

    /// Test temperature decision tree (non-exhaustive test)
    #[test]
    fn test_decide() {
        let dang_temp = ChainTemperature::Ok(150.0);
        let hot_temp = ChainTemperature::Ok(95.0);
        let low_temp = ChainTemperature::Ok(50.0);
        let temp_config = TempControlConfig {
            dangerous_temp: 100.0,
            hot_temp: 80.0,
        };
        let fan_speed = fan::Speed::new(50);
        let fan_config = FanControlConfig {
            mode: FanControlMode::FixedSpeed(fan_speed),
            min_fans: 2,
        };
        let fans_off = fan::Speed::STOPPED;
        let fans_off_config = Config {
            fans_on_while_warming_up: true,
            fan_config: Some(FanControlConfig {
                mode: FanControlMode::FixedSpeed(fans_off),
                min_fans: 2,
            }),
            temp_config: None,
        };
        let all_off_config = Config {
            fans_on_while_warming_up: true,
            fan_config: None,
            temp_config: None,
        };
        let fans_on_config = Config {
            fans_on_while_warming_up: true,
            fan_config: Some(fan_config.clone()),
            temp_config: None,
        };
        let temp_on_config = Config {
            fans_on_while_warming_up: true,
            fan_config: None,
            temp_config: Some(temp_config.clone()),
        };
        let both_on_config = Config {
            fans_on_while_warming_up: true,
            fan_config: Some(fan_config.clone()),
            temp_config: Some(temp_config.clone()),
        };
        let both_on_pid_config = Config {
            fans_on_while_warming_up: true,
            fan_config: Some(FanControlConfig {
                mode: FanControlMode::TargetTemperature(75.0),
                min_fans: 2,
            }),
            temp_config: Some(temp_config.clone()),
        };

        assert_variant!(
            ControlDecision::decide(&all_off_config, 0, dang_temp.clone()).decision,
            ControlDecision::Nothing
        );
        assert_variant!(
            ControlDecision::decide(&all_off_config, 0, ChainTemperature::Failed).decision,
            ControlDecision::Nothing
        );

        assert_eq!(
            ControlDecision::decide(&fans_on_config, 2, dang_temp.clone()).decision,
            ControlDecision::UseFixedSpeed(fan_speed)
        );
        assert_eq!(
            ControlDecision::decide(&fans_on_config, 0, dang_temp.clone()).decision,
            ControlDecision::Shutdown
        );
        assert_eq!(
            ControlDecision::decide(&fans_on_config, 1, dang_temp.clone()).decision,
            ControlDecision::Shutdown
        );
        assert_eq!(
            ControlDecision::decide(&fans_on_config, 2, ChainTemperature::Failed).decision,
            ControlDecision::UseFixedSpeed(fan_speed)
        );

        // fans set to 0 -> do not check if fans are running
        assert_eq!(
            ControlDecision::decide(&fans_off_config, 0, dang_temp.clone()).decision,
            ControlDecision::UseFixedSpeed(fans_off)
        );

        assert_eq!(
            ControlDecision::decide(&temp_on_config, 0, ChainTemperature::Failed).decision,
            ControlDecision::Shutdown
        );
        assert_variant!(
            ControlDecision::decide(&temp_on_config, 0, ChainTemperature::Unknown).decision,
            ControlDecision::Nothing
        );
        assert_eq!(
            ControlDecision::decide(&temp_on_config, 0, dang_temp).decision,
            ControlDecision::Shutdown
        );
        assert_variant!(
            ControlDecision::decide(&temp_on_config, 0, hot_temp).decision,
            ControlDecision::Nothing
        );

        assert_eq!(
            ControlDecision::decide(&both_on_config, 0, low_temp).decision,
            ControlDecision::Shutdown
        );
        assert_eq!(
            ControlDecision::decide(&both_on_config, 2, dang_temp).decision,
            ControlDecision::Shutdown
        );
        assert_eq!(
            ControlDecision::decide(&both_on_config, 2, ChainTemperature::Failed).decision,
            ControlDecision::Shutdown
        );
        assert_eq!(
            ControlDecision::decide(&both_on_config, 2, ChainTemperature::Unknown).decision,
            ControlDecision::UseFixedSpeed(fan::Speed::FULL_SPEED)
        );
        assert_eq!(
            ControlDecision::decide(&both_on_config, 2, hot_temp).decision,
            ControlDecision::UseFixedSpeed(fan_speed)
        );
        assert_eq!(
            ControlDecision::decide(&both_on_config, 2, low_temp).decision,
            ControlDecision::UseFixedSpeed(fan_speed)
        );

        assert_eq!(
            ControlDecision::decide(&both_on_pid_config, 0, low_temp).decision,
            ControlDecision::Shutdown
        );
        assert_eq!(
            ControlDecision::decide(&both_on_pid_config, 2, dang_temp).decision,
            ControlDecision::Shutdown
        );
        assert_eq!(
            ControlDecision::decide(&both_on_pid_config, 2, ChainTemperature::Failed).decision,
            ControlDecision::Shutdown
        );
        assert_eq!(
            ControlDecision::decide(&both_on_pid_config, 2, ChainTemperature::Unknown).decision,
            ControlDecision::UseFixedSpeed(fan::Speed::FULL_SPEED)
        );
        assert_eq!(
            ControlDecision::decide(&both_on_pid_config, 2, hot_temp).decision,
            ControlDecision::UseFixedSpeed(fan::Speed::FULL_SPEED)
        );
        assert_eq!(
            ControlDecision::decide(&both_on_pid_config, 2, low_temp).decision,
            ControlDecision::UsePid {
                target_temp: 75.0,
                input_temp: 50.0
            }
        );
    }
}
