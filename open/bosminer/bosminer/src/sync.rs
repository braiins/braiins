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

pub mod event;

use std::fmt;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, MutexGuard};

use atomic_enum::atomic_enum;

#[atomic_enum]
#[derive(PartialEq)]
pub enum Status {
    Created,
    Starting,
    Retrying,
    Running,
    Stopping,
    Failing,
    Declining,
    Restarting,
    Recovering,
    // TODO: Destroying
    Stopped,
    Failed,
    // TODO: Destroyed
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Monitor is intended for generic synchronization of node states
#[derive(Debug)]
pub struct StatusMonitor {
    status: AtomicStatus,
    start_stop_sync: Mutex<()>,
    event_sender: Mutex<Option<event::Sender>>,
}

impl StatusMonitor {
    #[inline]
    fn atomic_start_stop(&self) -> MutexGuard<()> {
        self.start_stop_sync
            .lock()
            .expect("BUG: cannot lock start/stop synchronization")
    }

    #[inline]
    pub fn status(&self) -> Status {
        self.status.load(Ordering::Relaxed)
    }

    /// `f` - closure that is 'run' to atomically perform actual start operation on an object if the
    /// internal state allows it
    pub fn initiate_starting(&self, f: impl FnOnce()) {
        let _sync = self.atomic_start_stop();

        let mut status = self.status();
        let mut run = false;

        loop {
            let previous = status;
            match status {
                Status::Created | Status::Stopped => {
                    status =
                        self.status
                            .compare_and_swap(status, Status::Starting, Ordering::Relaxed);
                    if status == previous {
                        // Starting has been initiated successfully
                        run = true;
                        break;
                    }
                }
                Status::Failed => {
                    status =
                        self.status
                            .compare_and_swap(status, Status::Retrying, Ordering::Relaxed);
                    if status == previous {
                        // Retrying has been initiated successfully
                        run = true;
                        break;
                    }
                }
                Status::Stopping => {
                    // Try to change state to `Restarting`
                    status =
                        self.status
                            .compare_and_swap(status, Status::Restarting, Ordering::Relaxed);
                    if status == previous {
                        break;
                    }
                }
                Status::Failing | Status::Declining => {
                    // Try to change state to `Recovering`
                    status =
                        self.status
                            .compare_and_swap(status, Status::Recovering, Ordering::Relaxed);
                    if status == previous {
                        break;
                    }
                }
                // Client is currently started
                Status::Starting
                | Status::Retrying
                | Status::Running
                | Status::Restarting
                | Status::Recovering => break,
            };
            // Try it again because another task change the state
        }
        if run {
            f();
        }
    }

    pub fn initiate_running(&self) -> bool {
        let mut status = self.status();

        loop {
            let previous = status;
            match status {
                Status::Created
                | Status::Running
                | Status::Stopped
                | Status::Failing
                | Status::Failed => panic!("BUG: 'report_fail': unexpected state '{:?}'", status),
                Status::Starting | Status::Retrying => {
                    status =
                        self.status
                            .compare_and_swap(status, Status::Running, Ordering::Relaxed);
                    if status == previous {
                        // Running has been set successfully
                        self.notify();
                        break;
                    }
                }
                Status::Stopping | Status::Declining | Status::Restarting | Status::Recovering => {
                    return false
                }
            }
            // Try it again because another task change the state
        }

        // Running can be done
        true
    }

    /// `f` - closure that is 'run' to atomically perform actual stop operation on an object
    /// if the internal state allows it
    pub fn initiate_stopping(&self, f: impl FnOnce()) {
        let _sync = self.atomic_start_stop();

        let mut status = self.status();
        let mut run = false;

        loop {
            let previous = status;
            match status {
                Status::Created
                | Status::Stopping
                | Status::Failing
                | Status::Declining
                | Status::Stopped
                | Status::Failed => break,
                // Client is currently started
                Status::Starting | Status::Running | Status::Restarting => {
                    status =
                        self.status
                            .compare_and_swap(status, Status::Stopping, Ordering::Relaxed);
                    if status == previous {
                        // Stopping has been initiated successfully
                        run = status != Status::Restarting;
                    }
                }
                Status::Retrying | Status::Recovering => {
                    status =
                        self.status
                            .compare_and_swap(status, Status::Declining, Ordering::Relaxed);
                    if status == previous {
                        // Stopping has been initiated successfully
                        run = status != Status::Recovering;
                    }
                }
            };
            // Try it again because another task change the state
        }
        if run {
            f();
        }
    }

    pub fn initiate_failing(&self) {
        let mut status = self.status();

        loop {
            let previous = status;
            match status {
                Status::Created | Status::Stopped | Status::Failed => {
                    panic!("BUG: 'report_fail': unexpected state '{:?}'", status)
                }
                Status::Running | Status::Stopping => {
                    status =
                        self.status
                            .compare_and_swap(status, Status::Failing, Ordering::Relaxed);
                    if status == previous {
                        // Failing has been set successfully
                        break;
                    }
                }
                Status::Starting | Status::Retrying => {
                    status =
                        self.status
                            .compare_and_swap(status, Status::Declining, Ordering::Relaxed);
                    if status == previous {
                        // Failing has been set successfully
                        break;
                    }
                }
                Status::Failing | Status::Declining | Status::Restarting | Status::Recovering => {
                    break
                }
            };
            // Try it again because another task change the state
        }
    }

    pub fn is_shutting_down(&self) -> bool {
        match self.status() {
            Status::Stopping | Status::Failing | Status::Declining => true,
            Status::Created
            | Status::Starting
            | Status::Running
            | Status::Retrying
            | Status::Restarting
            | Status::Recovering
            | Status::Stopped
            | Status::Failed => false,
        }
    }

    pub fn can_stop(&self) -> bool {
        let _sync = self.atomic_start_stop();

        let mut status = self.status();

        loop {
            let previous = status;
            match status {
                Status::Created
                | Status::Starting
                | Status::Retrying
                | Status::Running
                | Status::Stopped
                | Status::Failed => panic!("BUG: 'can_stop': unexpected state '{:?}'", status),
                Status::Stopping => {
                    // Try to change state to `Stopped`
                    status =
                        self.status
                            .compare_and_swap(status, Status::Stopped, Ordering::Relaxed);
                    if status == previous {
                        self.notify();
                        return true;
                    }
                }
                Status::Failing => {
                    // Try to change state to `Failed`
                    status =
                        self.status
                            .compare_and_swap(status, Status::Failed, Ordering::Relaxed);
                    if status == previous {
                        self.notify();
                        return true;
                    }
                }
                Status::Declining => {
                    // Try to change state to `Failed`
                    status =
                        self.status
                            .compare_and_swap(status, Status::Failed, Ordering::Relaxed);
                    if status == previous {
                        // Do not notify about repeated failures when status wasn't in running
                        // state before
                        return true;
                    }
                }
                // Restarting has been initiated
                Status::Restarting => {
                    // Try to change state to `Starting`
                    status =
                        self.status
                            .compare_and_swap(status, Status::Starting, Ordering::Relaxed);
                    if status == previous {
                        break;
                    }
                }
                // Recovering after previous fail has been initiated
                Status::Recovering => {
                    // Try to change state to `Retrying`
                    status =
                        self.status
                            .compare_and_swap(status, Status::Retrying, Ordering::Relaxed);
                    if status == previous {
                        break;
                    }
                }
            };
            // Try it again because another task change the state
        }

        // Stop cannot be done
        false
    }

    pub fn set_event_sender(&self, event_sender: event::Sender) -> Option<event::Sender> {
        self.event_sender
            .lock()
            .expect("BUG: cannot lock event sender for setting")
            .replace(event_sender)
    }

    pub fn take_event_sender(&self) -> Option<event::Sender> {
        self.event_sender
            .lock()
            .expect("BUG: cannot lock event sender for taking")
            .take()
    }

    fn notify(&self) {
        self.event_sender
            .lock()
            .expect("BUG: cannot lock event sender for notification")
            .as_ref()
            .map(|v| v.notify());
    }
}

impl Default for StatusMonitor {
    fn default() -> Self {
        Self {
            status: AtomicStatus::new(Status::Created),
            start_stop_sync: Mutex::new(()),
            event_sender: Mutex::new(None),
        }
    }
}
