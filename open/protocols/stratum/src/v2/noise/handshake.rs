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

//! Provides necessary infrastructure to run handshake on a noise framed stream

use bytes::BytesMut;
use snow::HandshakeState;
use std::time;

use ii_async_compat::prelude::*;

use crate::error::{Error, Result};

/// Handshake message
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Message {
    pub(super) inner: BytesMut,
}

impl Message {
    pub(super) fn new(inner: BytesMut) -> Self {
        Self { inner }
    }
}

/// Describes the step result what the relevant party should do after sending out the
/// provided message (if any)
#[derive(Debug, PartialEq)]
pub(super) enum StepResult {
    /// The object should receive a noise message and pass it for processing in the next step
    ReceiveMessage,
    /// The relevant party should send the provided message in this
    /// variant and expect a reply
    ExpectReply(Message),
    /// This message is yet to be sent to the counter party and we are allowed to switch to
    /// transport mode
    NoMoreReply(Message),
    /// The handshake is complete, no more messages are expected and nothing is to be sent. The
    /// protocol can be switched to transport mode now.
    Done,
}

/// Objects that can perform 1 handshake step implement this trait
pub(super) trait Step {
    /// Proceeds with the handshake and processes an optional incoming message - `in_msg` and
    /// generates a new handshake message to be sent out
    ///
    /// `in_msg` - optional input message to be processed
    /// `noise_bytes` - If this step generates an output message, it should be constructed into
    /// this buffer and returned as appropriate `StepResult`
    fn step(&mut self, in_msg: Option<Message>, noise_bytes: BytesMut) -> Result<StepResult>;

    /// Transforms step into the handshake state
    fn into_handshake_state(self) -> HandshakeState;
}

/// The purpose of this object is to interpret the `StepResult` instructions while driving the
/// inner handshake step object. This typically requires sending results down the noise stream
/// and receiving handshake messages. This is done until the handshake is complete or fails
pub(super) struct Handshake<T> {
    handshake_step: T,
}

impl<T> Handshake<T>
where
    T: Step,
{
    const HANDSHAKE_TIMEOUT: time::Duration = time::Duration::from_secs(2);

    pub(super) fn new(handshake_step: T) -> Self {
        Self { handshake_step }
    }

    /// Helper that receives 1 handshake message
    async fn receive_message(
        &self,
        handshake_stream: &mut super::NoiseFramedTcpStream,
    ) -> Result<Message> {
        let handshake_frame: BytesMut = handshake_stream
            .next()
            .timeout(Self::HANDSHAKE_TIMEOUT)
            .await?
            // Convert optional frame into an error, unwrap it, and unwrap the
            // payload, too
            .ok_or(Error::Handshake(
                "Noise handshake Connection shutdown".to_string(),
            ))??;
        Ok(Message::new(handshake_frame))
    }

    /// Consumes the handshake object and drives the inner `Step`
    pub(super) async fn run(
        mut self,
        handshake_stream: &mut super::NoiseFramedTcpStream,
    ) -> Result<super::TransportMode> {
        let mut in_msg: Option<Message> = None;

        loop {
            let handshake_buf: BytesMut = BytesMut::new();

            match self
                .handshake_step
                .step((&mut in_msg).take(), handshake_buf)?
            {
                // Just wait for an incoming handshake message
                StepResult::ReceiveMessage => {
                    let handshake_message = self.receive_message(handshake_stream).await?;
                    (&mut in_msg).replace(handshake_message);
                }
                // Send out specified messages and wait for response
                StepResult::ExpectReply(out_msg) => {
                    handshake_stream
                        .send(out_msg.inner)
                        .timeout(Self::HANDSHAKE_TIMEOUT)
                        .await??;

                    let handshake_message = self.receive_message(handshake_stream).await?;
                    (&mut in_msg).replace(handshake_message);
                }
                StepResult::NoMoreReply(out_msg) => {
                    handshake_stream
                        .send(out_msg.inner)
                        .timeout(Self::HANDSHAKE_TIMEOUT)
                        .await??;
                }
                // Initiator is now finalized
                StepResult::Done => {
                    break;
                }
            };
        }

        // At this point the handshake has been successfully completed
        // Consume the handshake step object extracting the `HandshakeState` and attempt to
        // transition into transport mode
        self.handshake_step
            .into_handshake_state()
            .into_transport_mode()
            .map_err(Into::into)
            .map(|t| super::TransportMode::new(t))
    }
}
