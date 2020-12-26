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

use std::collections::HashMap;
use std::convert::From;
use std::convert::TryFrom;
use std::convert::TryInto;
use std::fmt;
use std::mem::size_of;

use ii_async_compat::{bytes, futures};

use async_trait::async_trait;
use bytes::BytesMut;
use futures::channel::mpsc;

use bitcoin_hashes::{sha256d, Hash, HashEngine};
use serde_json;
use serde_json::Value;

use ii_stratum::v1;
use ii_stratum::v2::{
    self,
    types::{Bytes0_32, Str0_255, Uint256Bytes},
};

use ii_logging::macros::*;

use crate::error::{Error, Result, ResultExt};
use crate::util;

#[cfg(test)]
mod test;

/// Sequential ID to pair up messages, requests etc.
#[derive(Default, Debug)]
pub struct SeqId(u32);

impl SeqId {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a new ID, increment internal state
    pub fn next(&mut self) -> u32 {
        let current_value = self.0;
        self.0 = self.0.wrapping_add(1);
        current_value
    }
}

/// Compound struct for all translation options that can be tweaked in `V2ToV1Translation`
#[derive(Copy, Clone, Debug)]
pub struct V2ToV1TranslationOptions {
    /// Try to send `extranonce.subscribe` during handshake
    pub try_enable_xnsub: bool,
    /// Reconnect received from the upstream is translated and propagated to the v2 downstream
    /// connection. This can be useful for V2 clients that run this translation component locally
    pub propagate_reconnect_downstream: bool,
}

impl Default for V2ToV1TranslationOptions {
    fn default() -> Self {
        Self {
            try_enable_xnsub: false,
            propagate_reconnect_downstream: false,
        }
    }
}

/// States of the Translation setup
#[derive(PartialEq, Debug)]
enum V2ToV1TranslationState {
    /// No message received yet
    Init,
    /// Stratum V1 mining.configure is in progress
    V1Configure,
    /// Connection successfully setup, waiting for OpenStandardMiningChannel message
    ConnectionSetup,
    /// Channel now needs finalization of subscribe+authorize+set difficulty target with the
    /// upstream V1 server
    OpenStandardMiningChannelPending,
    /// Upstream subscribe/authorize failed state ensures sending OpenStandardMiningChannelError only once
    V1SubscribeOrAuthorizeFail,
    /// Channel is operational
    Operational,
}

/// Represents a handler method that can process a particular ii_stratum result.
type V1StratumResultHandler =
    fn(&mut V2ToV1Translation, &v1::MessageId, &v1::rpc::StratumResult) -> Result<()>;

/// Represents a handler method that can process a particular ii_stratum error.
type V1StratumErrorHandler =
    fn(&mut V2ToV1Translation, &v1::MessageId, &v1::rpc::StratumError) -> Result<()>;

type V1CompoundHandler = (V1StratumResultHandler, V1StratumErrorHandler);

/// Custom mapping of V1 request id onto result/error handlers
type V1ReqMap = HashMap<u32, V1CompoundHandler>;

/// Helper template stored in V2->V1 job map
#[derive(Clone, PartialEq, Debug)]
struct V1SubmitTemplate {
    job_id: v1::messages::JobId,
    time: u32,
    version: u32,
}

enum V1ResultOrError<'a> {
    Result(&'a v1::rpc::StratumResult),
    Error(&'a v1::rpc::StratumError),
}

/// Maps V2 job ID to V1 job ID so that we can submit mining results upstream to V1 server
type JobMap = HashMap<u32, V1SubmitTemplate>;

//type V2ReqMap = HashMap<u32, FnMut(&mut V2ToV1Translation, &ii_stratum::Message<Protocol>, &v1::rpc::StratumResult)>;

/// Object capable of translating stratm V2 header-only mining protocol that uses standard mining
/// channels into stratum V1 including extranonce 1 subscription
pub struct V2ToV1Translation {
    /// Statemachine tracking the translation setup
    state: V2ToV1TranslationState,

    /// Channel for sending out V1 responses
    v1_tx: mpsc::Sender<v1::Frame>,
    /// Unique request ID generator
    v1_req_id: SeqId,
    /// Mapping for pairing of incoming V1 message with original requests
    v1_req_map: V1ReqMap,

    v1_extra_nonce1: Option<v1::ExtraNonce1>,
    v1_extra_nonce2_size: usize,
    v1_authorized: bool,
    v1_xnsub_enabled: bool,

    /// Whether to force future jobs: might be handy for v1 pools which don't accept solutions with
    /// `ntime` less than specified on jobs they are solving (but greater than ntime on prevhash).
    v1_force_future_jobs: bool,

    /// Latest mining.notify payload that arrived before V1 authorize has completed.
    /// This allows immediate completion of channel open on V2.
    v1_deferred_notify: Option<v1::messages::Notify>,

    /// Channel for sending out V2 responses
    v2_tx: mpsc::Sender<v2::Frame>,
    #[allow(dead_code)] // TODO: unused as of now
    v2_req_id: SeqId,
    /// All connection details
    v2_conn_details: Option<v2::messages::SetupConnection>,
    /// Additional information about the pending channel being open
    v2_channel_details: Option<v2::messages::OpenStandardMiningChannel>,
    /// Target difficulty derived from mining.set_difficulty message
    /// The channel opening is not complete until the target is determined
    v2_target: Option<uint::U256>,
    /// Unique job ID generator
    v2_job_id: SeqId,
    /// Translates V2 job ID to V1 job ID
    v2_to_v1_job_map: JobMap,
    /// Options for translation
    options: V2ToV1TranslationOptions,
}

impl V2ToV1Translation {
    const PROTOCOL_VERSION: usize = 0;
    /// No support for the extended protocol yet, therefore, no extranonce advertised
    #[allow(dead_code)]
    const MAX_EXTRANONCE_SIZE: usize = 0;
    /// Currently, no support for multiple channels in the proxy
    const CHANNEL_ID: u32 = 0;
    /// Default group channel
    const DEFAULT_GROUP_CHANNEL_ID: u32 = 0;

    /// U256 in little endian
    /// TODO: consolidate into common part/generalize
    /// TODO: DIFF1 const target is broken, the last U64 word gets actually initialized to 0xffffffff, not sure why
    const DIFF1_TARGET: uint::U256 = uint::U256([0, 0, 0, 0xffff0000u64]);

    pub fn new(
        v1_tx: mpsc::Sender<v1::Frame>,
        v2_tx: mpsc::Sender<v2::Frame>,
        options: V2ToV1TranslationOptions,
    ) -> Self {
        Self {
            v2_conn_details: None,
            v2_channel_details: None,
            v2_target: None,
            state: V2ToV1TranslationState::Init,
            v1_tx,
            v1_req_id: SeqId::new(),
            v1_req_map: V1ReqMap::default(),
            v1_extra_nonce1: None,
            v1_extra_nonce2_size: 0,
            v1_authorized: false,
            v1_force_future_jobs: true,
            v1_xnsub_enabled: false,
            v1_deferred_notify: None,
            v2_tx,
            v2_req_id: SeqId::new(),
            v2_job_id: SeqId::new(),
            v2_to_v1_job_map: JobMap::default(),
            options,
        }
    }

    /// Builds a V1 request from V1 method and assigns a unique identifier to it
    fn v1_method_into_message<M, E>(
        &mut self,
        method: M,
        result_handler: V1StratumResultHandler,
        error_handler: V1StratumErrorHandler,
    ) -> v1::rpc::Rpc
    where
        E: fmt::Debug,
        M: TryInto<v1::rpc::RequestPayload, Error = E>,
    {
        let payload = method
            .try_into()
            .expect("BUG: Cannot convert V1 method into a message");

        // TODO: decorate the request with a new unique ID -> this is the request ID
        let id = self.v1_req_id.next();
        trace!("Registering v1, request ID: {} method: {:?}", id, payload);
        if self
            .v1_req_map
            .insert(id, (result_handler, error_handler))
            .is_some()
        {
            error!("BUG: V1 id {} already exists...", id);
            // TODO add graceful handling of this bug (shutdown?)
            panic!("V1 id already exists");
        }

        v1::rpc::Request {
            id: Some(id),
            payload,
        }
        .into()
    }

    /// Sets the current pending channel to operational state and submits success message
    fn finalize_open_channel(&mut self) -> Result<()> {
        trace!("finalize_open_channel()");
        let mut init_target: Uint256Bytes = Uint256Bytes([0; 32]);
        self.v2_target
            .expect("Bug: initial target still not defined when attempting to finalize OpenStandardMiningChannel")
            .to_little_endian(init_target.as_mut());

        // when V1 authorization has already taken place, report channel opening success
        if let Some(v2_channel_details) = self.v2_channel_details.as_ref() {
            self.state = V2ToV1TranslationState::Operational;
            let msg = v2::messages::OpenStandardMiningChannelSuccess {
                req_id: v2_channel_details.req_id,
                channel_id: Self::CHANNEL_ID,
                target: init_target.clone(),
                extranonce_prefix: Bytes0_32::new(),
                group_channel_id: Self::DEFAULT_GROUP_CHANNEL_ID,
            };
            util::submit_message(&mut self.v2_tx, msg)?;

            // If mining.notify is pending, process it now as part of open channel finalization
            if let Some(notify_payload) = self.v1_deferred_notify.take() {
                self.perform_notify(&notify_payload)?;
            }
            return Ok(());
        } else {
            Err(
                ii_stratum::error::Error::from(v2::error::ErrorKind::ChannelNotOperational(
                    "Channel details missing".to_string(),
                ))
                .into(),
            )
        }
    }

    /// Send new target
    /// TODO extend the translation unit test accordingly
    fn send_set_target(&mut self) -> Result<()> {
        trace!("send_set_target()");
        let max_target = Uint256Bytes::from(self.v2_target.expect(
            "Bug: initial target still not defined when attempting to finalize \
             OpenStandardMiningChannel",
        ));

        let msg = v2::messages::SetTarget {
            channel_id: Self::CHANNEL_ID,
            max_target,
        };

        util::submit_message(&mut self.v2_tx, msg)
    }

    /// Reports failure to open the channel and changes the translation state
    /// From this point on a new OpenStandardMiningChannel message is expected as an attempt to reopen the channel
    fn abort_open_channel(&mut self, err_msg: &str) {
        trace!(
            "abort_open_channel() - channel details: {:?}, msg: {}",
            self.v2_channel_details,
            err_msg
        );
        self.state = V2ToV1TranslationState::V1SubscribeOrAuthorizeFail;

        // Cleanup all parts associated with opening the channel
        self.v1_authorized = false;
        self.v1_extra_nonce1 = None;
        self.v1_extra_nonce2_size = 0;

        if let Some(v2_channel_details) = self.v2_channel_details.as_ref() {
            let msg = v2::messages::OpenStandardMiningChannelError {
                req_id: v2_channel_details.req_id,
                code: err_msg.try_into().expect("BUG: incorrect error message"),
            };
            self.v2_channel_details = None;

            if let Err(submit_err) = util::submit_message(&mut self.v2_tx, msg) {
                info!(
                    "abort_open_channel() failed: {:?}, abort message: {}",
                    submit_err, err_msg
                );
            }
        } else {
            error!(
                "abort_open_channel(): no channel to abort, missing V2 channel details, message: \
                 {}",
                err_msg
            );
        }
    }

    /// Finalizes a pending SetupConnection upon successful negotiation of
    /// mining configuration of version rolling bits
    fn handle_configure_result(
        &mut self,
        id: &v1::MessageId,
        payload: &v1::rpc::StratumResult,
    ) -> Result<()> {
        trace!(
            "handle_configure_result() id={:?} state={:?} payload:{:?}",
            id,
            self.state,
            payload,
        );

        // TODO review the use of serde_json here, it may be possible to eliminate this dependency
        // Extract version mask and verify it matches the maximum possible value
        let proposed_version_mask: v1::messages::VersionMask =
            serde_json::from_value(payload.0["version-rolling.mask"].clone())
                .context("Failed to parse version-rolling mask")
                .map_err(|e| ii_stratum::error::Error::from(e))?;

        trace!(
            "Evaluating: version-rolling state == {:?} && mask=={:x?}",
            payload.0["version-rolling"].as_bool(),
            proposed_version_mask
        );
        if payload.0["version-rolling"].as_bool() == Some(true)
            && (proposed_version_mask.0).0 == ii_stratum::BIP320_N_VERSION_MASK
        {
            self.state = V2ToV1TranslationState::ConnectionSetup;

            let success = v2::messages::SetupConnectionSuccess {
                used_version: Self::PROTOCOL_VERSION as u16,
                flags: 0,
            };
            util::submit_message(&mut self.v2_tx, success)
        } else {
            // TODO consolidate into abort_connection() + communicate shutdown of this
            // connection similarly everywhere in the code
            let response = v2::messages::SetupConnectionError {
                flags: 0, // TODO handle flags
                code: "Cannot negotiate upstream V1 version mask"
                    .try_into()
                    .expect("BUG: incorrect error message"),
            };
            util::submit_message(&mut self.v2_tx, response)
        }
    }

    fn handle_configure_error(
        &mut self,
        id: &v1::MessageId,
        payload: &v1::rpc::StratumError,
    ) -> Result<()> {
        trace!(
            "handle_configure_error() id={:?} state={:?} payload:{:?}",
            id,
            self.state,
            payload,
        );
        // TODO consolidate into abort_connection() + communicate shutdown of this
        // connection similarly everywhere in the code
        let response = v2::messages::SetupConnectionError {
            flags: 0, // TODO handle flags
            code: "Cannot negotiate upstream V1 version mask"
                .try_into()
                .expect("BUG: incorrect error message"),
        };
        util::submit_message(&mut self.v2_tx, response)
    }

    fn handle_extranonce_subscribe_result(
        &mut self,
        _id: &v1::MessageId,
        payload: &v1::rpc::StratumResult,
    ) -> Result<()> {
        v1::messages::BooleanResult::try_from(payload)
            // Convert potential stratum error to proxy error first
            .map_err(Into::into)
            // Handle the actual submission result
            .map(|bool_result| {
                if bool_result.0 {
                    info!("Support for #xnsub enabled");
                    self.v1_xnsub_enabled = true;
                } else {
                    error!("Pool refused to enable #xnsub");
                    self.v1_xnsub_enabled = false;
                }
                ()
            })
    }

    fn handle_extranonce_subscribe_error(
        &mut self,
        _id: &v1::MessageId,
        payload: &v1::rpc::StratumError,
    ) -> Result<()> {
        self.v1_xnsub_enabled = false;
        error!("Error when trying to enable #xnsub: {}", payload.1);
        Ok(())
    }

    fn handle_subscribe_result(
        &mut self,
        id: &v1::MessageId,
        payload: &v1::rpc::StratumResult,
    ) -> Result<()> {
        trace!(
            "handle_subscribe_result() id={:?} state={:?} payload:{:?}",
            id,
            self.state,
            payload,
        );
        let subscribe_result = v1::messages::SubscribeResult::try_from(payload).map_err(|e| {
            // Aborting channel failed, we can only log about it
            self.abort_open_channel("Upstream subscribe failed");
            e
        })?;

        self.v1_extra_nonce1 = Some(subscribe_result.extra_nonce_1().clone());
        self.v1_extra_nonce2_size = subscribe_result.extra_nonce_2_size().clone();

        // In order to finalize the opening procedure we need 3 items: authorization,
        // subscription and difficulty
        if self.v1_authorized && self.v2_target.is_some() {
            self.finalize_open_channel().map_err(|e| {
                self.abort_open_channel("Upstream subscribe failed");
                e
            })?
        }
        Ok(())
    }

    /// An authorize result should be true, any other problem results in aborting the channel
    fn handle_authorize_result(
        &mut self,
        id: &v1::MessageId,
        payload: &v1::rpc::StratumResult,
    ) -> Result<()> {
        trace!(
            "handle_authorize_result() id={:?} state={:?} payload:{:?}",
            id,
            self.state,
            payload,
        );
        // Authorize is expected as a plain boolean answer
        v1::messages::BooleanResult::try_from(payload)
            // Convert ii-stratum error to proxy error
            .map_err(Into::into)
            .and_then(|bool_result| {
                trace!("Authorize result: {:?}", bool_result);
                self.v1_authorized = bool_result.0;
                if self.v1_authorized {
                    // Subscribe result already received (since extra nonce 1 is present), let's
                    // finalize the open channel
                    if self.v1_extra_nonce1.is_some() && self.v2_target.is_some() {
                        self.finalize_open_channel()
                    }
                    // Channel opening will be finalized by subscribe result
                    else {
                        Ok(())
                    }
                } else {
                    Err(
                        ii_stratum::error::Error::from(v1::error::ErrorKind::Subscribe(
                            "Authorize result is false".to_string(),
                        ))
                        .into(),
                    )
                }
            })
            // any problem in parsing the response results in authorization failure
            .map_err(|e| {
                self.abort_open_channel("Not authorized");
                e
            })
    }

    fn handle_authorize_or_subscribe_error(
        &mut self,
        id: &v1::MessageId,
        payload: &v1::rpc::StratumError,
    ) -> Result<()> {
        trace!(
            "handle_authorize_or_subscribe_error() id={:?} state={:?} payload:{:?}",
            id,
            self.state,
            payload,
        );
        // Only the first of authorize or subscribe error issues the OpenStandardMiningChannelError message
        if self.state != V2ToV1TranslationState::V1SubscribeOrAuthorizeFail {
            trace!(
                "Upstream connection init failed, dropping channel: {:?}",
                payload
            );
            self.abort_open_channel("Service not ready");
            Err(Error::from(ii_stratum::error::Error::from(
                v1::error::ErrorKind::Subscribe(format!("{:?}", payload)),
            )))
        } else {
            trace!("Ok, received the second of subscribe/authorize failures, channel is already closed: {:?}", payload);
            Ok(())
        }
    }

    fn handle_submit_result(
        &mut self,
        id: &v1::MessageId,
        payload: &v1::rpc::StratumResult,
    ) -> Result<()> {
        trace!(
            "handle_submit_result() id={:?} state={:?} payload:{:?}",
            id,
            self.state,
            payload,
        );
        // mining.submit response is expected as a plain boolean answer
        v1::messages::BooleanResult::try_from(payload)
            // Convert potential stratum error to proxy error first
            .map_err(Into::into)
            // Handle the actual submission result
            .and_then(|bool_result| {
                let v2_channel_details = self
                    .v2_channel_details
                    .as_ref()
                    .expect("BUG: V2 channel details missing");
                trace!(
                    "Submit result: {:?}, V2 channel: {:?}",
                    bool_result,
                    v2_channel_details
                );
                if bool_result.0 {
                    // TODO this is currently incomplete, we have to track all pending mining
                    // results so that we can correlate the success message and ack
                    let success_msg = v2::messages::SubmitSharesSuccess {
                        channel_id: Self::CHANNEL_ID,
                        last_seq_num: 0,
                        new_submits_accepted_count: 1,
                        new_shares_sum: 1, // TODO is this really 1?
                    };
                    self.log_session_details("Share accepted");
                    util::submit_message(&mut self.v2_tx, success_msg)
                } else {
                    // TODO use reject_shares() method once we can track the original payload message
                    let err_msg = v2::messages::SubmitSharesError {
                        channel_id: Self::CHANNEL_ID,
                        // TODO the sequence number needs to be determined from the failed submit, currently,
                        // there is no infrastructure to get this
                        seq_num: 0,
                        code: format!("ShareRjct:{:?}", payload)[..32]
                            .try_into()
                            .expect("BUG: incorrect error message"),
                    };
                    info!("Share rejected for {}", v2_channel_details.user.to_string());
                    util::submit_message(&mut self.v2_tx, err_msg)
                }
            })
            // TODO what should be the behavior when the result is incorrectly passed, shall we
            // report it as a SubmitSharesError?
            .map_err(Into::into)
    }

    fn handle_submit_error(
        &mut self,
        id: &v1::MessageId,
        payload: &v1::rpc::StratumError,
    ) -> Result<()> {
        trace!(
            "handle_submit_error() id={:?} state={:?} payload:{:?}",
            id,
            self.state,
            payload,
        );
        // TODO use reject_shares() method once we can track the original payload message
        let err_msg = v2::messages::SubmitSharesError {
            channel_id: Self::CHANNEL_ID,
            // TODO the sequence number needs to be determined from the failed submit, currently,
            // there is no infrastructure to get this
            seq_num: 0,
            code: format!("ShareRjct:{:?}", payload)[..32]
                .try_into()
                .expect("BUG: wrong error code string"),
        };

        util::submit_message(&mut self.v2_tx, err_msg)
    }

    /// Iterates the merkle branches and calculates block merkle root using the extra nonce 1.
    /// Extra nonce 2 encodes the channel ID.
    /// TODO review, whether a Result has to be returned as missing enonce1 would be considered a bug
    fn calculate_merkle_root(
        &mut self,
        payload: &v1::messages::Notify,
    ) -> crate::error::Result<sha256d::Hash> {
        // TODO get rid of extra nonce 1 cloning
        if let Some(v1_extra_nonce1) = self.v1_extra_nonce1.clone() {
            // Build coin base transaction,
            let mut coin_base: BytesMut = BytesMut::with_capacity(
                payload.coin_base_1().len()
                    + (v1_extra_nonce1.0).len()
                    + self.v1_extra_nonce2_size
                    + payload.coin_base_2().len(),
            );
            coin_base.extend_from_slice(payload.coin_base_1());
            coin_base.extend_from_slice(v1_extra_nonce1.0.as_ref());
            coin_base.extend_from_slice(
                Self::channel_to_extra_nonce2_bytes(Self::CHANNEL_ID, self.v1_extra_nonce2_size)
                    .as_ref(),
            );
            coin_base.extend_from_slice(payload.coin_base_2());

            let mut engine = sha256d::Hash::engine();
            engine.input(&coin_base);

            let cb_tx_hash = sha256d::Hash::from_engine(engine);
            trace!("Coinbase TX hash: {:x?} {:x?}", cb_tx_hash, coin_base);

            let merkle_root =
                payload
                    .merkle_branch()
                    .iter()
                    .fold(cb_tx_hash, |curr_merkle_root, tx_hash| {
                        let mut engine = sha256d::Hash::engine();
                        engine.input(&curr_merkle_root.into_inner());
                        engine.input(tx_hash.as_ref().as_slice());
                        sha256d::Hash::from_engine(engine)
                    });
            trace!("Merkle root calculated: {:x?}", merkle_root);
            Ok(merkle_root)
        } else {
            Err(super::error::ErrorKind::General(
                "Extra nonce 1 missing, cannot calculate merkle root".into(),
            )
            .into())
        }
    }

    /// Builds SetNewPrevHash for the specified v1 Notify `payload`
    ///
    /// The SetNewPrevHash has to reference the future job that the V2 downstream has
    /// previously received from us.
    ///
    /// TODO: how to and if at all shall we determine the ntime offset? The proxy, unless
    /// it has access to the bitcoin network, cannot determine precisly determine the median
    /// ntime. Using sys_time may not be suitable:
    /// max_ntime_offset = min(min_ntime + 7200, sys_time + 7200)- min_ntime =
    ///                  = 7200 - min(0, sys_time - min_ntime)
    ///
    /// To be safe, we won't go for 7200, and suggest something like 1/4 of the calculated
    /// interval
    fn build_set_new_prev_hash(
        &self,
        job_id: u32,
        payload: &v1::messages::Notify,
    ) -> crate::error::Result<v2::messages::SetNewPrevHash> {
        // TODO review how this can be prevented from failing. If this fails, it should result in
        // panic as it marks a software bug
        let prev_hash =
            sha256d::Hash::from_slice(payload.prev_hash()).context("Build SetNewPrevHash")?;
        let prev_hash = Uint256Bytes(prev_hash.into_inner());

        Ok(v2::messages::SetNewPrevHash {
            channel_id: Self::CHANNEL_ID,
            prev_hash,
            min_ntime: payload.time(),
            nbits: payload.bits(),
            job_id,
        })
    }

    /// Converts specified `channel_id` into extra nonce 2 with a specified
    /// `v1_extra_nonce2_size`
    /// TODO review the implementation 'how to efficiently render a u32 into a byte array'
    #[inline]
    fn channel_to_extra_nonce2_bytes(channel_id: u32, v1_extra_nonce2_size: usize) -> BytesMut {
        let mut extra_nonce2: BytesMut = BytesMut::with_capacity(v1_extra_nonce2_size);

        let channel_id_bytes = u32::to_le_bytes(channel_id);
        if v1_extra_nonce2_size < size_of::<u32>() {
            // TODO: what to do when server deliberately sends small extranonce?
            if channel_id >= 1u32.wrapping_shl(8 * v1_extra_nonce2_size as u32) {
                error!("BUG: channel_id doesn't fit into extranonce");
            }
            // Write just part of channel_id
            extra_nonce2.extend_from_slice(&channel_id_bytes[0..v1_extra_nonce2_size]);
        } else {
            // Write full 32-bits of channel id and pad the rest
            extra_nonce2.extend_from_slice(&channel_id_bytes);
            let padding = v1_extra_nonce2_size - size_of::<u32>();
            extra_nonce2.extend_from_slice(&vec![0; padding]);
        }
        extra_nonce2
    }

    /// Generates log trace entry and reject shares error reply to the client
    fn reject_shares(&mut self, payload: &v2::messages::SubmitSharesStandard, err_msg: String) {
        trace!("Unrecognized channel ID: {}", payload.channel_id);
        let submit_shares_error_msg = v2::messages::SubmitSharesError {
            channel_id: payload.channel_id,
            seq_num: payload.seq_num,
            code: err_msg[..32].try_into().expect(
                format!(
                    "BUG: cannot convert error message to V2 format: {}",
                    err_msg
                )
                .as_str(),
            ),
        };

        if let Err(submit_err) = util::submit_message(&mut self.v2_tx, submit_shares_error_msg) {
            info!("Cannot send 'SubmitSharesError': {:?}", submit_err);
        }
    }

    fn perform_notify(&mut self, payload: &v1::messages::Notify) -> Result<()> {
        let merkle_root = self.calculate_merkle_root(payload)?;

        let v2_job = v2::messages::NewMiningJob {
            channel_id: Self::CHANNEL_ID,
            job_id: self.v2_job_id.next(),
            future_job: self.v2_to_v1_job_map.is_empty()
                || payload.clean_jobs()
                || self.v1_force_future_jobs,
            merkle_root: Uint256Bytes(merkle_root.into_inner()),
            version: payload.version(),
        };

        // Make sure we generate new prev hash. Empty JobMap means this is the first mining.notify
        // message and we also have to issue NewPrevHash. In addition to that, we also check the
        // clean jobs flag that indicates a must for new prev hash, too.
        let maybe_set_new_prev_hash = if v2_job.future_job {
            self.v2_to_v1_job_map.clear();
            // Any error means immediate termination
            // TODO write a unit test for such scenario, too
            Some(self.build_set_new_prev_hash(v2_job.job_id, payload)?)
        } else {
            None
        };
        trace!(
            "Registering V2 job ID {:x?} -> V1 job ID {:x?}",
            v2_job.job_id,
            payload.job_id(),
        );
        // TODO extract this duplicate code, turn the map into a new type with this
        // custom policy (attempt to insert with the same key is a bug)
        if self
            .v2_to_v1_job_map
            .insert(
                v2_job.job_id,
                V1SubmitTemplate {
                    job_id: v1::messages::JobId::from_str(payload.job_id()),
                    time: payload.time(),
                    version: payload.version(),
                },
            )
            .is_some()
        {
            error!("BUG: V2 id {} already exists...", v2_job.job_id);
            // TODO add graceful handling of this bug (shutdown?)
            panic!("V2 id already exists");
        }

        util::submit_message(&mut self.v2_tx, v2_job)?;

        if let Some(set_new_prev_hash) = maybe_set_new_prev_hash {
            util::submit_message(&mut self.v2_tx, set_new_prev_hash)?
        }
        Ok(())
    }

    /// The result visitor takes care of detecting a spurious response without matching request
    /// and passes processing further
    /// TODO write a solid unit test covering all 3 scenarios that can go wrong
    async fn visit_stratum_result_or_error<'a>(
        &mut self,
        id: &'a v1::MessageId,
        payload: V1ResultOrError<'a>,
    ) {
        // Each response message should have an ID for pairing
        id.ok_or(Error::from(ii_stratum::error::Error::from(
            v1::error::ErrorKind::Rpc("Missing ID in ii_stratum result".to_string()),
        )))
        // find the ID in the request map
        .and_then(|id| {
            self.v1_req_map
                .remove(&id)
                .ok_or(Error::from(ii_stratum::error::Error::from(
                    v1::error::ErrorKind::Rpc(format!("Received invalid ID {}", id).into()),
                )))
        })
        // run the result through the result handler
        .and_then(|handler| match payload {
            V1ResultOrError::Result(r) => handler.0(self, id, r),
            V1ResultOrError::Error(e) => handler.1(self, id, e),
        })
        .map_err(|e| info!("visit_stratum_result failed: {}", e))
        // Consume the error as there is no way to return anything from the visitor for now.
        .ok();
    }

    /// Parse the stratum V1 reconnect message into new host/port pair, where host is
    /// converted into stratum v2 specific type. This method catches host name overflow attempts.
    fn parse_client_reconnect(msg: &v1::messages::ClientReconnect) -> Result<(Str0_255, u16)> {
        let new_host = match msg.host() {
            Some(host_val) => match host_val {
                Value::String(host_name) => Ok(host_name.clone()),
                _ => Err("host name not a string"),
            },
            None => Ok("".to_owned()),
        }
        .and_then(|host_name| {
            // TODO Str0_255 conversion returns () as an error, therefore we cannot mention the
            // error here. Once this changes, the error mapping can be redone
            Str0_255::try_from(host_name).map_err(|_e| "host name string too long")
        })
        .map_err(|e| {
            crate::error::ErrorKind::General(format!(
                "Cannot parse host ({}) in client.reconnect: {:?}",
                e, msg
            ))
        })?;

        let new_port = match msg.port() {
            Some(port_val) => match port_val {
                Value::Number(port) => match port.as_u64() {
                    Some(n) => u16::try_from(n).map_err(|_e| "invalid u16"),
                    None => Err("invalid u16"),
                },
                Value::String(port_str) => port_str
                    .parse::<u16>()
                    .map_err(|_e| "invalid number string"),
                _ => Err("port number neither string nor int"),
            },
            None => Ok(0),
        }
        .map_err(|e| {
            crate::error::ErrorKind::General(format!(
                "Cannot parse port ({}) client.reconnect: {:?}",
                e, msg
            ))
        })?;

        Ok((new_host, new_port))
    }

    fn log_session_details(&self, msg: &str) {
        let v2_channel_details = self
            .v2_channel_details
            .as_ref()
            .expect("BUG: V2 channel details missing");
        let v2_connection_details = self
            .v2_conn_details
            .as_ref()
            .expect("BUG: V2 channel details present but connection details missing?");
        info!(
            "{} SESSION;{};{};{};{};{:x};{};{};{};{};{};{};",
            msg,
            v2_channel_details.user.to_string(),
            v2_connection_details.protocol,
            v2_connection_details.min_version,
            v2_connection_details.max_version,
            v2_connection_details.flags,
            v2_connection_details.endpoint_host.to_string(),
            v2_connection_details.endpoint_port,
            v2_connection_details.device.vendor.to_string(),
            v2_connection_details.device.hw_rev.to_string(),
            v2_connection_details.device.fw_ver.to_string(),
            v2_connection_details.device.dev_id.to_string(),
        );
    }
}

#[async_trait]
impl v1::Handler for V2ToV1Translation {
    async fn visit_stratum_result(&mut self, id: &v1::MessageId, payload: &v1::rpc::StratumResult) {
        trace!(
            "visit_stratum_result() id={:?} state={:?} payload:{:?}",
            id,
            self.state,
            payload,
        );
        self.visit_stratum_result_or_error(id, V1ResultOrError::Result(payload))
            .await;
    }

    async fn visit_stratum_error(&mut self, id: &v1::MessageId, payload: &v1::rpc::StratumError) {
        trace!(
            "visit_stratum_error() id={:?} state={:?} payload:{:?}",
            id,
            self.state,
            payload,
        );
        self.visit_stratum_result_or_error(id, V1ResultOrError::Error(payload))
            .await;
    }

    async fn visit_set_difficulty(
        &mut self,
        id: &v1::MessageId,
        payload: &v1::messages::SetDifficulty,
    ) {
        trace!(
            "visit_set_difficulty() id={:?} state={:?} payload:{:?}",
            id,
            self.state,
            payload,
        );
        let diff = payload.value() as u32;
        self.v2_target = Some(Self::DIFF1_TARGET / diff);
        if self.v1_authorized && self.v1_extra_nonce1.is_some() {
            // Initial set difficulty finalizes open channel if all preconditions are met
            if self.state == V2ToV1TranslationState::OpenStandardMiningChannelPending {
                self.finalize_open_channel()
                    .map_err(|e| trace!("visit_set_difficulty: {}", e))
                    // Consume the error as there is no way to return anything from the visitor for now.
                    .ok();
            }
            // Anything after that is standard difficulty adjustment
            else {
                trace!("Sending current target: {:x?}", self.v2_target);
                if let Err(e) = self.send_set_target() {
                    info!("Cannot send SetTarget: {}", e);
                }
            }
        }
    }

    async fn visit_set_extranonce(
        &mut self,
        id: &v1::MessageId,
        payload: &v1::messages::SetExtranonce,
    ) {
        trace!(
            "visit_set_extranonce() id={:?} state={:?} payload:{:?}",
            id,
            self.state,
            payload,
        );

        // Update extranonces.
        // Changes are reflected after new mining job as per:
        //   https://en.bitcoin.it/wiki/Stratum_mining_protocol#mining.set_extranonce
        self.v1_extra_nonce1 = Some(payload.extra_nonce_1().clone());
        self.v1_extra_nonce2_size = payload.extra_nonce_2_size().clone();
    }

    /// Composes a new mining job and sends it downstream
    /// TODO: Only 1 channel is supported
    async fn visit_notify(&mut self, id: &v1::MessageId, payload: &v1::messages::Notify) {
        trace!(
            "visit_notify() id={:?} state={:?} payload:{:?}",
            id,
            self.state,
            payload,
        );

        // We won't process the job as long as the channel is not operational
        if self.state != V2ToV1TranslationState::Operational {
            self.v1_deferred_notify = Some(payload.clone());
            debug!("Channel not yet operational, caching latest mining.notify from upstream");
            return;
        }
        self.perform_notify(payload)
            .map_err(|e| {
                info!(
                    "visit_notify: Sending new mining job failed error={:?} id={:?} state={:?} \
                     payload:{:?}",
                    e, id, self.state, payload,
                )
            })
            // Consume the error as there is no way this can be communicated further
            .ok();
    }

    /// TODO currently unimplemented, the proxy should refuse changing the version mask from the server
    /// Since this is a notification only, the only action that the translation can do is log +
    /// report an error
    async fn visit_set_version_mask(
        &mut self,
        id: &v1::MessageId,
        payload: &v1::messages::SetVersionMask,
    ) {
        trace!(
            "visit_set_version_mask() id={:?} state={:?} payload:{:?}",
            id,
            self.state,
            payload,
        );
    }

    async fn visit_client_reconnect(
        &mut self,
        id: &v1::MessageId,
        payload: &v1::messages::ClientReconnect,
    ) {
        trace!(
            "visit_client_reconnect() id={:?} state={:?} payload:{:?}",
            id,
            self.state,
            payload,
        );
        // Propagate the reconnect only if configured so
        if self.options.propagate_reconnect_downstream {
            Self::parse_client_reconnect(payload)
                .map(|(new_host, new_port)| {
                    let reconnect_msg = v2::messages::Reconnect { new_host, new_port };

                    if let Err(submit_err) = util::submit_message(&mut self.v2_tx, reconnect_msg) {
                        info!("Cannot send 'Reconnect': {:?}", submit_err);
                    };
                })
                .map_err(|e| info!("visit_client_reconnect failed: {}", e))
                // Consume the result, no way to return a status from the visitor
                .ok();
        }
    }
}

/// TODO: implement an internal state where in each state only a subset of visit methods is valid,
/// the rest of the methods have default implementation that only reports error in the log and to the client, dropping a connection?
/// Connection dropping is to be clarified
#[async_trait]
impl v2::Handler for V2ToV1Translation {
    async fn visit_setup_connection(
        &mut self,
        header: &v2::framing::Header,
        payload: &v2::messages::SetupConnection,
    ) {
        trace!(
            "visit_setup_mining_connection() header={:x?} state={:?} payload:{:?}",
            header,
            self.state,
            payload,
        );
        if self.state != V2ToV1TranslationState::Init {
            trace!("Cannot setup connection again, received: {:?}", payload);
            let err_msg = v2::messages::SetupConnectionError {
                code: "Connection can be setup only once"
                    .try_into()
                    .expect("BUG: incorrect error message"),
                flags: payload.flags, // TODO Flags indicating features causing an error
            };
            if let Err(submit_err) = util::submit_message(&mut self.v2_tx, err_msg) {
                info!("Cannot submit SetupConnectionError: {:?}", submit_err);
                return;
            }
        }

        self.v2_conn_details = Some(payload.clone());
        let mut configure = v1::messages::Configure::new();
        configure
            .add_feature(v1::messages::VersionRolling::new(
                ii_stratum::BIP320_N_VERSION_MASK,
                ii_stratum::BIP320_N_VERSION_MAX_BITS,
            ))
            .expect("addfeature failed"); // FIXME: how to handle errors from configure.add_feature() ?

        let v1_configure_message = self.v1_method_into_message(
            configure,
            Self::handle_configure_result,
            Self::handle_configure_error,
        );
        if let Err(submit_err) = util::submit_message(&mut self.v1_tx, v1_configure_message) {
            info!("Cannot submit mining.configure: {:?}", submit_err);
            return;
        }
        self.state = V2ToV1TranslationState::V1Configure;
    }

    /// Opening a channel is a 2 stage process when translating to  V1 ii_stratum, where
    /// both stages can be executed in arbitrary order:
    /// - perform subscribe (and start queuing incoming V1 jobs)
    /// - perform authorize
    ///
    /// Upon successful authorization:
    /// - communicate OpenStandardMiningChannelSuccess
    /// - start sending Jobs downstream to V2 client
    async fn visit_open_standard_mining_channel(
        &mut self,
        header: &v2::framing::Header,
        payload: &v2::messages::OpenStandardMiningChannel,
    ) {
        trace!(
            "visit_open_standard_mining_channel() header={:x?} state={:?} payload:{:?}",
            header,
            self.state,
            payload,
        );
        if self.state != V2ToV1TranslationState::ConnectionSetup
            && self.state != V2ToV1TranslationState::V1SubscribeOrAuthorizeFail
        {
            trace!(
                "Out of sequence OpenStandardMiningChannel message, received: {:?}",
                payload
            );
            let err_msg = v2::messages::OpenStandardMiningChannelError {
                req_id: payload.req_id,
                code: "Out of sequence OpenStandardMiningChannel msg"
                    .try_into()
                    .expect("BUG: incorrect error message"),
            };

            if let Err(submit_err) = util::submit_message(&mut self.v2_tx, err_msg) {
                info!(
                    "Cannot send OpenStandardMiningChannelError message: {:?}",
                    submit_err
                );
                return;
            }
        }
        // Connection details are present by now
        if let Some(conn_details) = self.v2_conn_details.as_ref() {
            self.v2_channel_details = Some(payload.clone());
            self.state = V2ToV1TranslationState::OpenStandardMiningChannelPending;

            let hostname: String = conn_details
                .endpoint_host
                .clone()
                .try_into()
                .expect("BUG: Cannot convert to string from connection details");

            let hostname_port = format!("{}:{}", hostname, conn_details.endpoint_port);
            let subscribe = v1::messages::Subscribe(
                Some(conn_details.device.fw_ver.to_string()),
                None,
                Some(hostname_port),
                None,
            );

            let v1_subscribe_message = self.v1_method_into_message(
                subscribe,
                Self::handle_subscribe_result,
                Self::handle_authorize_or_subscribe_error,
            );

            if let Err(submit_err) = util::submit_message(&mut self.v1_tx, v1_subscribe_message) {
                info!("Cannot send V1 mining.subscribe: {:?}", submit_err);
                return;
            }

            if self.options.try_enable_xnsub {
                let extranonce_subscribe = v1::messages::ExtranonceSubscribe();
                let v1_extranonce_subscribe = self.v1_method_into_message(
                    extranonce_subscribe,
                    Self::handle_extranonce_subscribe_result,
                    Self::handle_extranonce_subscribe_error,
                );
                if let Err(submit_err) =
                    util::submit_message(&mut self.v1_tx, v1_extranonce_subscribe)
                {
                    info!(
                        "Cannot send V1 mining.extranonce_subscribe: {:?}",
                        submit_err
                    );
                    return;
                }
            }

            let authorize = v1::messages::Authorize(payload.user.to_string(), "".to_string());
            let v1_authorize_message = self.v1_method_into_message(
                authorize,
                Self::handle_authorize_result,
                Self::handle_authorize_or_subscribe_error,
            );
            if let Err(submit_err) = util::submit_message(&mut self.v1_tx, v1_authorize_message) {
                info!("Cannot send V1 mining.authorized: {:?}", submit_err);
                return;
            }
        }
    }

    /// The flow of share processing is as follows:
    ///
    /// - find corresponding job
    /// - verify the share that it meets the target (NOTE: currently not implemented)
    /// - emit V1 Submit message
    ///
    /// If any of the above points fail, reply with SubmitShareError + reasoning
    async fn visit_submit_shares_standard(
        &mut self,
        header: &v2::framing::Header,
        payload: &v2::messages::SubmitSharesStandard,
    ) {
        trace!(
            "visit_submit_shares() header={:x?} state={:?} payload:{:?}",
            header,
            self.state,
            payload,
        );
        // Report invalid channel ID
        if payload.channel_id != Self::CHANNEL_ID {
            self.reject_shares(
                payload,
                format!("Unrecognized channel ID {}", payload.channel_id),
            );
            return;
        }

        // Channel details must be filled by now, anything else is a bug, unfortunately, due to
        // the 'expect' we have to clone them. TODO review this code
        let v2_channel_details = &self
            .v2_channel_details
            .clone()
            .expect("Missing channel details");
        // TODO this is only here as we want to prevent locking up 'self' into multiple closures
        // and causing borrow checker complains
        let v1_extra_nonce2_size = self.v1_extra_nonce2_size;

        // Check job ID validity
        let v1_submit_template = self
            .v2_to_v1_job_map
            .get(&payload.job_id)
            // convert missing job ID (None) into an error
            .ok_or(crate::error::ErrorKind::General(format!(
                "V2 Job ID not present {} in registry",
                payload.job_id
            )))
            .map(|tmpl| tmpl.clone());
        // TODO validate the job (recalculate the hash and compare the target)
        // Submit upstream V1 job based on the found job ID in the map
        match v1_submit_template {
            Ok(v1_submit_template) => {
                let submit = v1::messages::Submit::new(
                    v2_channel_details.user.to_string(),
                    v1_submit_template.job_id.clone(),
                    Self::channel_to_extra_nonce2_bytes(Self::CHANNEL_ID, v1_extra_nonce2_size)
                        .as_ref(),
                    payload.ntime,
                    payload.nonce,
                    // ensure the version bits in the template follow BIP320
                    payload.version & ii_stratum::BIP320_N_VERSION_MASK,
                );
                // Convert the method into a message + provide handling methods
                let v1_submit_message = self.v1_method_into_message(
                    submit,
                    Self::handle_submit_result,
                    Self::handle_submit_error,
                );
                if let Err(submit_err) = util::submit_message(&mut self.v1_tx, v1_submit_message) {
                    info!(
                        "SubmitSharesStandard: cannot send translated V1 message: {:?}",
                        submit_err
                    );
                }
            }
            Err(e) => self.reject_shares(payload, format!("{}", e)),
        }
    }
}
