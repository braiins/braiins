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

//! Stratum version 2 top level module
pub mod error;
pub mod framing;
#[macro_use]
pub mod macros;
pub mod extensions;
pub mod messages;
pub mod noise;
pub mod serialization;
pub mod telemetry;
pub mod types;

use self::messages::MessageType;
use crate::error::Result;
use crate::{AnyPayload, Message};

use async_trait::async_trait;
use packed_struct::prelude::*;
use std::convert::TryFrom;
use tokio::net::TcpStream;

use ii_async_compat::prelude::*;
use ii_logging::macros::*;
use ii_wire;

pub use self::framing::codec::Codec;
pub use self::framing::{Frame, Framing};

/// Tcp stream that produces/consumes V2 frames
pub type Framed = tokio_util::codec::Framed<TcpStream, <Framing as ii_wire::Framing>::Codec>;

pub trait FramedSink:
    Sink<<Framing as ii_wire::Framing>::Tx, Error = <Framing as ii_wire::Framing>::Error>
    + std::marker::Unpin
    + std::fmt::Debug
    + 'static
{
}

impl<T> FramedSink for T where
    T: Sink<<Framing as ii_wire::Framing>::Tx, Error = <Framing as ii_wire::Framing>::Error>
        + std::marker::Unpin
        + std::fmt::Debug
        + 'static
{
}

/// Helper type for outgoing V2 frames when run time support for multiple sink types (e.g.
/// TcpStream, mpsc::Sender etc.) is needed
pub type DynFramedSink = std::pin::Pin<
    Box<
        dyn Sink<<Framing as ii_wire::Framing>::Tx, Error = <Framing as ii_wire::Framing>::Error>
            + Send
            + std::marker::Unpin,
    >,
>;

pub trait FramedStream:
    Stream<
        Item = std::result::Result<
            <Framing as ii_wire::Framing>::Tx,
            <Framing as ii_wire::Framing>::Error,
        >,
    > + std::marker::Unpin
    + 'static
{
}

impl<T> FramedStream for T where
    T: Stream<
            Item = std::result::Result<
                <Framing as ii_wire::Framing>::Tx,
                <Framing as ii_wire::Framing>::Error,
            >,
        > + std::marker::Unpin
        + 'static
{
}

/// Helper type for incoming V2 frames when run time support for multiple sources (e.g.
/// TcpStream, mpsc::Receiver etc.) is needed
pub type DynFramedStream = std::pin::Pin<
    Box<
        dyn Stream<
                Item = std::result::Result<
                    <Framing as ii_wire::Framing>::Rx,
                    <Framing as ii_wire::Framing>::Error,
                >,
            > + Send,
    >,
>;

/// Protocol associates a custom handler with it
pub struct Protocol;
impl crate::Protocol for Protocol {
    type Handler = dyn Handler;
    type Header = framing::Header;
}

/// Specifies all messages to be visited
/// TODO document why anything implementing the Handler must be static
#[async_trait]
pub trait Handler: 'static + Send {
    async fn visit_setup_connection(
        &mut self,
        _header: &framing::Header,
        _payload: &messages::SetupConnection,
    ) {
    }

    async fn visit_setup_connection_success(
        &mut self,
        _header: &framing::Header,
        _payload: &messages::SetupConnectionSuccess,
    ) {
    }

    async fn visit_setup_connection_error(
        &mut self,
        _header: &framing::Header,
        _payload: &messages::SetupConnectionError,
    ) {
    }

    async fn visit_open_standard_mining_channel(
        &mut self,
        _header: &framing::Header,
        _payload: &messages::OpenStandardMiningChannel,
    ) {
    }

    async fn visit_open_standard_mining_channel_success(
        &mut self,
        _header: &framing::Header,
        _payload: &messages::OpenStandardMiningChannelSuccess,
    ) {
    }

    async fn visit_open_standard_mining_channel_error(
        &mut self,

        _header: &framing::Header,
        _payload: &messages::OpenStandardMiningChannelError,
    ) {
    }

    async fn visit_update_channel(
        &mut self,
        _header: &framing::Header,
        _payload: &messages::UpdateChannel,
    ) {
    }

    async fn visit_update_channel_error(
        &mut self,
        _header: &framing::Header,
        _payload: &messages::UpdateChannelError,
    ) {
    }

    async fn visit_submit_shares_standard(
        &mut self,
        _header: &framing::Header,
        _payload: &messages::SubmitSharesStandard,
    ) {
    }

    async fn visit_submit_shares_success(
        &mut self,
        _header: &framing::Header,
        _payload: &messages::SubmitSharesSuccess,
    ) {
    }

    async fn visit_submit_shares_error(
        &mut self,
        _header: &framing::Header,
        _payload: &messages::SubmitSharesError,
    ) {
    }

    async fn visit_new_mining_job(
        &mut self,
        _header: &framing::Header,
        _payload: &messages::NewMiningJob,
    ) {
    }

    async fn visit_set_new_prev_hash(
        &mut self,
        _header: &framing::Header,
        _payload: &messages::SetNewPrevHash,
    ) {
    }

    async fn visit_reconnect(&mut self, _header: &framing::Header, _payload: &messages::Reconnect) {
    }

    async fn visit_set_target(
        &mut self,
        _header: &framing::Header,
        _payload: &messages::SetTarget,
    ) {
    }

    // TODO the methods below will be removed once we will split off a separate handler
    //  type for the telemetry extension and refactor message handling completely
    async fn visit_open_telemetry_channel(
        &mut self,
        _header: &framing::Header,
        _payload: &telemetry::messages::OpenTelemetryChannel,
    ) {
    }

    async fn visit_open_telemetry_channel_success(
        &mut self,
        _header: &framing::Header,
        _payload: &telemetry::messages::OpenTelemetryChannelSuccess,
    ) {
    }

    async fn visit_open_telemetry_channel_error(
        &mut self,
        _header: &framing::Header,
        _payload: &telemetry::messages::OpenTelemetryChannelError,
    ) {
    }

    async fn visit_submit_telemetry_data(
        &mut self,
        _header: &framing::Header,
        _payload: &telemetry::messages::SubmitTelemetryData,
    ) {
    }

    async fn visit_submit_telemetry_data_success(
        &mut self,
        _header: &framing::Header,
        _payload: &telemetry::messages::SubmitTelemetryDataSuccess,
    ) {
    }

    async fn visit_submit_telemetry_data_error(
        &mut self,
        _header: &framing::Header,
        _payload: &telemetry::messages::SubmitTelemetryDataError,
    ) {
    }
}

/// Consumes `frame` and produces a Message object based on the payload type
pub fn build_message_from_frame(frame: framing::Frame) -> Result<Message<Protocol>> {
    trace!("V2: building message from frame {:x?}", frame);

    // Payload that already contains deserialized message can be returned directly
    if frame.payload.is_serializable() {
        let (header, payload) = frame.split();
        let serializable_payload = payload
            .into_serializable()
            .expect("BUG: cannot convert payload into serializable");

        return Ok(Message {
            header,
            payload: serializable_payload,
        });
    }
    // Header will be consumed by the subsequent transformation of the frame into the actual
    // payload for further handling. Therefore we create a copy for constructing a
    // Message<Protocol >
    let header = frame.header.clone();
    // Deserialize the payload;h based on its type specified in the header
    let payload: Box<dyn AnyPayload<Protocol>> = match MessageType::from_primitive(
        frame.header.msg_type,
    )
    .ok_or(error::ErrorKind::UnknownMessage(
        format!("Unexpected payload type, full header: {:x?}", frame.header).into(),
    ))? {
        MessageType::SetupConnection => Box::new(messages::SetupConnection::try_from(frame)?),
        MessageType::SetupConnectionSuccess => {
            Box::new(messages::SetupConnectionSuccess::try_from(frame)?)
        }
        MessageType::SetupConnectionError => {
            Box::new(messages::SetupConnectionError::try_from(frame)?)
        }
        MessageType::OpenStandardMiningChannel => {
            Box::new(messages::OpenStandardMiningChannel::try_from(frame)?)
        }
        MessageType::OpenStandardMiningChannelSuccess => {
            Box::new(messages::OpenStandardMiningChannelSuccess::try_from(frame)?)
        }
        MessageType::OpenStandardMiningChannelError => {
            Box::new(messages::OpenStandardMiningChannelError::try_from(frame)?)
        }
        MessageType::NewMiningJob => Box::new(messages::NewMiningJob::try_from(frame)?),
        MessageType::SetNewPrevHash => Box::new(messages::SetNewPrevHash::try_from(frame)?),
        MessageType::Reconnect => Box::new(messages::Reconnect::try_from(frame)?),
        MessageType::SetTarget => Box::new(messages::SetTarget::try_from(frame)?),
        MessageType::SubmitSharesStandard => {
            Box::new(messages::SubmitSharesStandard::try_from(frame)?)
        }
        MessageType::SubmitSharesSuccess => {
            Box::new(messages::SubmitSharesSuccess::try_from(frame)?)
        }
        MessageType::SubmitSharesError => Box::new(messages::SubmitSharesError::try_from(frame)?),
        _ => {
            return Err(error::ErrorKind::UnknownMessage(
                format!("Unexpected payload type, full header: {:?}", frame.header).into(),
            )
            .into())
        }
    };

    Ok(Message { header, payload })
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::test_utils::v2::*;

    use ii_async_compat::tokio;
    use std::convert::TryInto;

    /// This test demonstrates an actual implementation of protocol handler (aka visitor to a set of
    /// desired messsages
    /// TODO refactor this test once we have a message dispatcher in place
    #[tokio::test]
    async fn test_build_message_from_frame() {
        let message_payload = build_setup_connection();
        let frame = message_payload
            .try_into()
            .expect("Cannot create test frame");

        let message =
            build_message_from_frame(frame).expect("Message payload deserialization failed");
        message.accept(&mut TestIdentityHandler).await;
    }
}
