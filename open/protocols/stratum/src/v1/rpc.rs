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

//! This module defines Stratum V1 JSON-RPC.
//! Eventhough, the protocol is pure JSON it distinguishes 2 types of frames with different
//! JSON scheme:
//! - stratum request (with optional ID), request with NULL ID is considered a notification.
//! - stratum response (associated with a previously issued request by the ID)

use async_trait::async_trait;
use serde;
use serde::{de, Deserialize, Serialize};
use serde_json;
use serde_json::Value;
use std::convert::TryFrom;
use std::result::Result as StdResult;
use std::str::FromStr;

use super::{framing, Handler, Protocol};
use crate::error::{Error, Result, ResultExt};
use crate::AnyPayload;

/// All recognized methods of the V1 protocol have the 'mining.' prefix in json.
#[derive(Serialize, Debug, PartialEq)]
pub enum Method {
    #[serde(rename = "mining.subscribe")]
    Subscribe,
    #[serde(rename = "mining.extranonce.subscribe")]
    ExtranonceSubscribe,
    #[serde(rename = "mining.authorize")]
    Authorize,
    #[serde(rename = "mining.set_difficulty")]
    SetDifficulty,
    #[serde(rename = "mining.set_extranonce")]
    SetExtranonce,
    #[serde(rename = "mining.configure")]
    Configure,
    #[serde(rename = "mining.submit")]
    Submit,
    #[serde(rename = "mining.notify")]
    Notify,
    #[serde(rename = "mining.set_version_mask")]
    SetVersionMask,
    #[serde(rename = "client.reconnect")]
    ClientReconnect,
    /// Catch all variant
    Unknown(String),
}

impl<'de> de::Deserialize<'de> for Method {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        use Method::*;

        let method = String::deserialize(deserializer)?;
        let method = match method.as_str() {
            "mining.subscribe" => Subscribe,
            "mining.extranonce.subscribe" => ExtranonceSubscribe,
            "mining.authorize" => Authorize,
            "mining.set_difficulty" => SetDifficulty,
            "mining.set_extranonce" => SetExtranonce,
            "mining.configure" => Configure,
            "mining.submit" => Submit,
            "mining.notify" => Notify,
            "mining.set_version_mask" => SetVersionMask,
            "client.reconnect" => ClientReconnect,
            _ => Unknown(method),
        };

        Ok(method)
    }
}

/// The motivation is to provide only the payload part of the message to ID
/// handling
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct RequestPayload {
    /// Protocol method to be 'called'
    pub method: Method,
    /// Vector of method parameters
    pub params: Value,
}

/// Generic stratum request
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct Request {
    /// Optional identifier for pairing with the response. Empty ID is a notification
    pub id: Option<u32>,

    /// Request payload doesn't have special tag, we need to separate it to simplify
    /// serialization/deserialization
    #[serde(flatten)]
    pub payload: RequestPayload,
}

/// New type that represents stratum result that can be further parsed based on the actual
/// response type
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct StratumResult(pub serde_json::Value);

impl StratumResult {
    pub fn new_from<T: Serialize>(value: T) -> Result<Self> {
        let value = serde_json::to_value(value).context("Failed to convert to result")?;
        Ok(Self(value))
    }
}

// Note: this doesn't work due to conflicting implementation from core (
// ```impl<T, U> std::convert::TryFrom<U> for T
//            where U: std::convert::Into<T>; ```
//
//impl<T: Serialize> TryFrom<T> for StratumResult {
//    type Error = crate::error::Error;
//
//    fn try_from(value: T) -> std::result::Result<Self, Self::Error> {
//        let value = serde_json::to_value(value).context("Failed to convert to result")?;
//        StratumResult(value)
//    }
//}

/// Specific protocol implementation for any stratum result
#[async_trait::async_trait]
impl AnyPayload<Protocol> for StratumResult {
    async fn accept(&self, id: &<Protocol as crate::Protocol>::Header, handler: &mut dyn Handler) {
        handler.visit_stratum_result(id, self).await;
    }

    fn serialize_to_writer(&self, _writer: &mut dyn std::io::Write) -> Result<()> {
        panic!(
            "BUG: serialization of partial message without Rpc not supported {:?}",
            self
        );
    }
}

#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct StratumError(pub i32, pub String, pub Option<String>);
// TODO this currently doesn't compile. Investigate serde_tuple issue.
//#[derive(Serialize_tuple, Deserialize_tuple, Debug)]
//pub struct StratumError {
//    pub code: i32,
//    pub msg: String,
//    pub trace_back: Option<String>,
//}

/// Specific protocol implementation for any stratum error
#[async_trait::async_trait]
impl AnyPayload<Protocol> for StratumError {
    async fn accept(
        &self,
        id: &<Protocol as crate::Protocol>::Header,
        handler: &mut <Protocol as crate::Protocol>::Handler,
    ) {
        handler.visit_stratum_error(id, self).await;
    }

    fn serialize_to_writer(&self, _writer: &mut dyn std::io::Write) -> Result<()> {
        panic!(
            "BUG: serialization of partial message without Rpc not supported {:?}",
            self
        );
    }
}

/// The motivation is to provide only the payload part of the message to ID
/// handling
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct ResponsePayload {
    /// Successful responses/notification have a result
    pub result: Option<StratumResult>,
    /// Error responses provide details via this field
    pub error: Option<StratumError>,
}

/// Generic stratum response
///
/// The response maybe optionally paired via 'id' with original request. Empty ID
/// represents a notification.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct Response {
    /// Response pairing identifier
    pub id: u32,
    /// Response payload doesn't have special tag, we need to separate it to simplify
    /// serialization/deserialization
    #[serde(flatten)]
    pub payload: ResponsePayload,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(untagged)]
pub enum Rpc {
    Request(Request),
    Response(Response),
}

impl From<Request> for Rpc {
    fn from(req: Request) -> Self {
        Rpc::Request(req)
    }
}

impl From<Response> for Rpc {
    fn from(resp: Response) -> Self {
        Rpc::Response(resp)
    }
}

impl TryFrom<&[u8]> for Rpc {
    type Error = Error;

    fn try_from(frame: &[u8]) -> Result<Self> {
        let x = serde_json::from_slice(&frame)?;
        Ok(x)
    }
}

impl TryFrom<Rpc> for framing::Frame {
    type Error = Error;
    /// Prepares a frame for serializing the specified message just in time (the message is
    /// treated as a `SerializablePayload`)
    fn try_from(m: Rpc) -> Result<Self> {
        Ok(framing::Frame::from_serializable_payload(m))
    }
}

impl TryFrom<framing::Frame> for Rpc {
    type Error = Error;

    fn try_from(frame: framing::Frame) -> Result<Self> {
        let payload = frame.into_inner();
        let payload = payload.into_bytes_mut()?;
        Self::try_from(&payload[..])
    }
}

impl FromStr for Rpc {
    type Err = Error;

    /// Any error is being converted into JSON parsing error
    #[inline]
    fn from_str(s: &str) -> Result<Self> {
        let x = serde_json::from_str(s).context("Parsing JSON failed")?;
        Ok(x)
    }
}

/// Each RPC is a `SerializablePayload` object that can be serialized into `writer` on demand
#[async_trait]
impl AnyPayload<Protocol> for Rpc {
    /// This will never get used as we don't do any handling on Rpc level. Since the RPC is also a
    /// SerializablePayload we have to provide a default implementation.
    async fn accept(
        &self,
        _id: &<Protocol as crate::Protocol>::Header,
        _handler: &mut <Protocol as crate::Protocol>::Handler,
    ) {
        panic!("BUG: Cannot accept handler for generic RPC {:?}", self);
    }

    fn serialize_to_writer(&self, writer: &mut dyn std::io::Write) -> Result<()> {
        serde_json::to_writer(writer, self).map_err(Into::into)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::v1::*;
    use bytes::BytesMut;
    use ii_async_compat::bytes;
    use std::convert::TryInto;

    #[test]
    fn test_deserialize_serialize_request() {
        for &req in V1_TEST_REQUESTS {
            let deserialized_request = Rpc::try_from(req.as_bytes()).unwrap();

            let request_frame: framing::Frame = deserialized_request
                .try_into()
                .expect("Failed to serialize");
            let mut serialized_request = BytesMut::new();

            request_frame
                .serialize(&mut serialized_request)
                .expect("Cannot serialize frame");

            assert_eq!(
                req,
                std::str::from_utf8(&serialized_request[..]).expect("UTF-8 from serialized frame"),
                "Rpc requests don't match"
            );
        }
    }

    #[test]
    fn test_deserialize_broken_request() {
        let deserialized = Rpc::try_from(MINING_BROKEN_REQ_JSON.as_bytes()).unwrap();

        match deserialized {
            Rpc::Request(request) => assert_eq!(
                Method::Unknown("mining.none_existing".into()),
                request.payload.method,
                "Unknown method not detected!"
            ),
            Rpc::Response(resp) => assert!(false, "Unexpected response: {:x?}", resp),
        }
    }

    fn test_deserialize_response(serialized_response: &str, expected_rpc: Rpc) {
        let deserialized_response =
            Rpc::try_from(serialized_response.as_bytes()).expect("Cannot deserialize JSON request");

        assert_eq!(
            expected_rpc, deserialized_response,
            "Stratum responses don't match!"
        );
    }
    #[test]
    fn test_deserialize_ok_response() {
        test_deserialize_response(
            MINING_SUBSCRIBE_OK_RESULT_JSON,
            build_subscribe_ok_response_message(),
        );
    }

    #[test]
    fn test_deserialize_err_response() {
        test_deserialize_response(STRATUM_ERROR_JSON, build_stratum_err_response());
    }

    /// Helper function that runs the serialization test on arbitrary response
    fn test_serialize_response(response: Rpc, expected_serialized_response: &str) {
        let response_frame: framing::Frame = response.try_into().expect("Failed to serialize");
        let mut serialized_response = BytesMut::new();

        response_frame
            .serialize(&mut serialized_response)
            .expect("Cannot serialize frame");

        assert_eq!(
            expected_serialized_response,
            std::str::from_utf8(&serialized_response[..]).expect("UTF-8 from serialized frame"),
            "Serializing test request yields different results!"
        );
    }

    #[test]
    fn test_serialize_ok_response() {
        test_serialize_response(
            build_subscribe_ok_response_message(),
            MINING_SUBSCRIBE_OK_RESULT_JSON,
        );
    }

    /// Verifies correct implementation of `SerializablePayload` trait for `Rpc`
    #[test]
    fn test_serialize_err_response() {
        test_serialize_response(build_stratum_err_response(), STRATUM_ERROR_JSON);
    }
}
