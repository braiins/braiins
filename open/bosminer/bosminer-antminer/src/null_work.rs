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

use ii_bitcoin::HashTrait;

use bosminer::job::{self, Bitcoin};
use bosminer::node;
use bosminer::stats;
use bosminer::sync;
use bosminer::work;
use bosminer_macros::ClientNode;

use std::fmt;
use std::sync::{Arc, Weak};

use async_trait::async_trait;

use once_cell::sync::Lazy;

static NULL_JOB_CLIENT: Lazy<Arc<NullJobClient>> = Lazy::new(|| Arc::new(NullJobClient::new()));

/// NullJob to be used for chip initialization and tests
#[derive(Debug, Copy, Clone)]
pub struct NullJob {
    hash: ii_bitcoin::DHash,
    time: u32,
    bits: u32,
    version: u32,
    target: ii_bitcoin::Target,
}

impl NullJob {
    /// XXX: maybe create a structure with named members to pass to this constructor, otherwise it's confusing.
    pub fn new(time: u32, bits: u32, version: u32) -> Self {
        Self {
            hash: ii_bitcoin::DHash::from_slice(&[0xffu8; 32]).unwrap(),
            time,
            bits,
            version,
            target: Default::default(),
        }
    }

    pub fn next(&mut self) {
        self.time += 1;
    }
}

#[derive(Debug, ClientNode)]
struct NullJobClient {
    #[member_status]
    status: sync::StatusMonitor,
    #[member_client_stats]
    client_stats: stats::BasicClient,
}

impl NullJobClient {
    pub fn new() -> Self {
        Self {
            status: Default::default(),
            client_stats: Default::default(),
        }
    }
}

// TODO: Remove dependency on this trait in backend
#[async_trait]
impl node::Client for NullJobClient {
    fn start(self: Arc<Self>) {}

    fn stop(&self) {}

    async fn get_last_job(&self) -> Option<Arc<dyn job::Bitcoin>> {
        None
    }
}

impl fmt::Display for NullJobClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Antminer NULL job generator")
    }
}

impl job::Bitcoin for NullJob {
    fn origin(&self) -> Weak<dyn node::Client> {
        Arc::downgrade(&(NULL_JOB_CLIENT.clone() as Arc<dyn node::Client>))
    }

    fn version(&self) -> u32 {
        self.version
    }

    fn version_mask(&self) -> u32 {
        0
    }

    fn previous_hash(&self) -> &ii_bitcoin::DHash {
        &self.hash
    }

    fn merkle_root(&self) -> &ii_bitcoin::DHash {
        &self.hash
    }

    fn time(&self) -> u32 {
        self.time
    }

    fn bits(&self) -> u32 {
        self.bits
    }

    fn target(&self) -> ii_bitcoin::Target {
        self.target
    }

    fn is_valid(&self) -> bool {
        true
    }
}

/// * `i` - unique identifier for the generated midstate
pub fn prepare(i: u64) -> work::Assignment {
    let job = Arc::new(NullJob::new(0, 0xffff_ffff, 0));
    let time = job.time();

    let mut midstate_bytes = [0u8; ii_bitcoin::SHA256_DIGEST_SIZE];
    midstate_bytes[..std::mem::size_of::<u64>()].copy_from_slice(&u64::to_le_bytes(i));

    let mid = work::Midstate {
        version: 0,
        state: midstate_bytes.into(),
    };

    work::Assignment::new(job, vec![mid], time)
}

pub fn prepare_opencore(enable_core: bool, midstate_count: usize) -> work::Assignment {
    let bits = if enable_core { 0xffff_ffff } else { 0 };
    let job = Arc::new(NullJob::new(0, bits, 0));
    let time = job.time();

    let one_midstate = work::Midstate {
        version: 0,
        state: [0u8; ii_bitcoin::SHA256_DIGEST_SIZE].into(),
    };

    work::Assignment::new(job, vec![one_midstate; midstate_count], time)
}
