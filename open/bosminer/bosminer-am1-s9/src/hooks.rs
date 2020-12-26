// Copyright (C) 2020  Braiins Systems s.r.o.
//
// This file is part of Braiins Initiative for Open-Source (BIOS).
//
// BIOS is free software: you can redistribute it and/or modify
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
// Please, keep in mind that we may also license BIOS or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

use crate::halt;
use crate::monitor;
use crate::Manager;

use bosminer::client;
use ii_cgminer_api::command;

use std::fmt::Debug;
use std::sync::Arc;

use async_trait::async_trait;

/// Trait to be implemented by external creates wishing extending functionality of the bare miner
#[async_trait]
pub trait Hooks: Send + Sync + Debug {
    /// Called when halt pair was created and registered into global halt context
    ///
    /// * `miner_halt_sender` is a parent halt context that knows how to shutdown the whole miner
    async fn halt_created(
        &self,
        _sender: Arc<halt::Sender>,
        _receiver: halt::Receiver,
        _miner_halt_sender: Arc<halt::Sender>,
    ) {
    }

    /// Called when `Monitor` has been started
    async fn monitor_started(&self, _monitor: Arc<monitor::Monitor>) {}

    /// Called when init process is about to start hash chain via `Manager`.
    /// Called for each hashchain.
    /// Return value: `true` if init should start hashchain, `false` otherwise.
    async fn can_start_chain(&self, manager: Arc<Manager>) -> bool {
        return manager.chain_config.enabled;
    }

    /// Called after miner has been started
    async fn miner_started(&self) {}

    /// Called after all groups with clients has been loaded
    async fn clients_loaded(&self, _client_manager: client::Manager) {}

    /// Called after custom commands creation
    async fn cgminer_custom_commands_created(
        self: Arc<Self>,
        _cgminer_commands: &mut command::Map,
    ) {
    }
}

/// NoHooks uses default implementation of all hooks
#[derive(Debug)]
pub struct NoHooks;

impl Hooks for NoHooks {}
