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

pub mod error;
pub mod framing;
pub mod messages;
pub mod rpc;

use self::error::ErrorKind;
pub use self::framing::codec::Codec;
pub use self::framing::{Frame, Framing};
use self::rpc::*;
use crate::error::{Result, ResultExt};
use crate::{AnyPayload, Message};

use async_trait::async_trait;
use bitcoin_hashes::hex::{FromHex, ToHex};
use byteorder::{BigEndian, ByteOrder, LittleEndian, WriteBytesExt};
use hex::FromHexError;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::mem::size_of;
use tokio::net::TcpStream;

use ii_async_compat::prelude::{tokio, tokio_util};
use ii_logging::macros::*;

//// Tcp stream that produces/consumes V2 frames
pub type Framed = tokio_util::codec::Framed<TcpStream, <Framing as ii_wire::Framing>::Codec>;

// Message Id is used for pairing request/response messages
/// TODO spread this type across the protocol and eliminate the `Option<u32>`
pub type MessageId = Option<u32>;

/// Protocol associates a custom handler with it
pub struct Protocol;
impl crate::Protocol for Protocol {
    type Handler = dyn Handler;
    /// Simplified protocol 'header' really contains only an optional message ID
    type Header = MessageId;
}

/// Specifies all messages to be visited
#[async_trait]
pub trait Handler: 'static + Send {
    /// Handles the result part of the response
    async fn visit_stratum_result(&mut self, _id: &MessageId, _payload: &rpc::StratumResult) {}
    /// Handles the error part of the response
    async fn visit_stratum_error(&mut self, _id: &MessageId, _payload: &rpc::StratumError) {}

    async fn visit_configure(&mut self, _id: &MessageId, _payload: &messages::Configure) {}

    async fn visit_subscribe(&mut self, _id: &MessageId, _payload: &messages::Subscribe) {}

    async fn visit_extranonce_subscribe(
        &mut self,
        _id: &MessageId,
        _payload: &messages::ExtranonceSubscribe,
    ) {
    }

    async fn visit_set_extranonce(&mut self, _id: &MessageId, _payload: &messages::SetExtranonce) {}

    async fn visit_authorize(&mut self, _id: &MessageId, _payload: &messages::Authorize) {}

    async fn visit_set_difficulty(&mut self, _id: &MessageId, _payload: &messages::SetDifficulty) {}

    async fn visit_notify(&mut self, _id: &MessageId, _payload: &messages::Notify) {}

    async fn visit_set_version_mask(
        &mut self,
        _id: &MessageId,
        _payload: &messages::SetVersionMask,
    ) {
    }

    async fn visit_submit(&mut self, _id: &MessageId, _payload: &messages::Submit) {}

    async fn visit_client_reconnect(
        &mut self,
        _id: &MessageId,
        _payload: &messages::ClientReconnect,
    ) {
    }
}

pub fn build_message_from_frame(frame: framing::Frame) -> Result<Message<Protocol>> {
    // TODO extend try_from to implement downcast from SerializablePayload/AnyPayload to Rpc (that
    //  implements SerializablePayload, too).
    let rpc = Rpc::try_from(frame)?;

    // The rest of the deserialization needs to be take care of also in case the Rpc was present
    // in the frame in deserialized form.
    trace!("V1: Deserialized V1 message payload: {:?}", rpc);
    let (id, payload) = match rpc {
        Rpc::Request(request) => {
            let id = request.id;
            let payload = match request.payload.method {
                Method::Configure => Box::new(messages::Configure::try_from(request)?)
                    as Box<dyn AnyPayload<Protocol>>,
                Method::Subscribe => Box::new(messages::Subscribe::try_from(request)?)
                    as Box<dyn AnyPayload<Protocol>>,
                Method::Submit => {
                    Box::new(messages::Submit::try_from(request)?) as Box<dyn AnyPayload<Protocol>>
                }
                Method::Authorize => Box::new(messages::Authorize::try_from(request)?)
                    as Box<dyn AnyPayload<Protocol>>,
                Method::SetDifficulty => Box::new(messages::SetDifficulty::try_from(request)?)
                    as Box<dyn AnyPayload<Protocol>>,
                Method::SetExtranonce => Box::new(messages::SetExtranonce::try_from(request)?)
                    as Box<dyn AnyPayload<Protocol>>,
                Method::Notify => {
                    Box::new(messages::Notify::try_from(request)?) as Box<dyn AnyPayload<Protocol>>
                }
                Method::SetVersionMask => Box::new(messages::SetVersionMask::try_from(request)?)
                    as Box<dyn AnyPayload<Protocol>>,
                Method::ClientReconnect => Box::new(messages::ClientReconnect::try_from(request)?)
                    as Box<dyn AnyPayload<Protocol>>,
                _ => {
                    return Err(ErrorKind::Rpc(format!("Unsupported request {:?}", request)).into())
                }
            };
            (id, payload)
        }
        // This is not ideal implementation as we clone() the result or error parts of the response.
        // Note, however, the unwrap() is safe as the error/result are 'Some'
        Rpc::Response(response) => {
            let id = response.id;
            let msg = if response.payload.error.is_some() {
                Box::new(response.payload.error.unwrap().clone()) as Box<dyn AnyPayload<Protocol>>
            } else if response.payload.result.is_some() {
                Box::new(response.payload.result.unwrap().clone()) as Box<dyn AnyPayload<Protocol>>
            } else {
                return Err(ErrorKind::Rpc(format!(
                    "Malformed response no error, no result specified {:?}",
                    response
                ))
                .into());
            };
            (Some(id), msg)
        }
    };
    trace!("V1: Deserialized message ID {:?}", id);
    Ok(Message {
        header: id,
        payload,
    })
}

/// Extranonce 1 introduced as new type to provide shared conversions to/from string
/// TODO: find out correct byte order for extra nonce 1
/// TODO: implement deref trait consolidate use of extra nonce 1
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ExtraNonce1(pub HexBytes);

/// Helper type that allows simple serialization and deserialization of byte vectors
/// that are represented as hex strings in JSON
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(into = "String", from = "String")]
pub struct HexBytes(Vec<u8>);

impl HexBytes {
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// Referencing the internal part of hex bytes
impl AsRef<Vec<u8>> for HexBytes {
    fn as_ref(&self) -> &Vec<u8> {
        &self.0
    }
}

/// fix for error on odd-length hex sequences
/// FIXME: find a nicer solution
fn hex_decode(s: &str) -> std::result::Result<Vec<u8>, FromHexError> {
    if s.len() % 2 != 0 {
        hex::decode(&format!("0{}", s))
    } else {
        hex::decode(s)
    }
}

impl TryFrom<&str> for HexBytes {
    type Error = crate::error::Error;

    fn try_from(value: &str) -> Result<Self> {
        Ok(HexBytes(
            hex_decode(value).context("Parsing hex bytes failed")?,
        ))
    }
}

/// TODO: this is not the cleanest way as any deserialization error is essentially consumed and
/// manifested as empty vector. However, it is very comfortable to use this trait implementation
/// in Extranonce1 serde support
impl From<String> for HexBytes {
    fn from(value: String) -> Self {
        HexBytes::try_from(value.as_str()).unwrap_or(HexBytes(vec![]))
    }
}

/// Helper Serializer
impl Into<String> for HexBytes {
    fn into(self) -> String {
        hex::encode(self.0)
    }
}

/// PrevHash in Stratum V1 has brain-damaged serialization as it swaps bytes of very u32 word
/// into big endian. Therefore, we need a special type for it
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(into = "String", from = "String")]
pub struct PrevHash(Vec<u8>);

impl PrevHash {
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// Referencing the internal part of the prev hash
impl AsRef<Vec<u8>> for PrevHash {
    fn as_ref(&self) -> &Vec<u8> {
        &self.0
    }
}

/// TODO: implement unit test
impl TryFrom<&str> for PrevHash {
    type Error = crate::error::Error;

    fn try_from(value: &str) -> Result<Self> {
        // Reorder prevhash will be stored via this cursor
        let mut prev_hash_cursor = std::io::Cursor::new(Vec::new());

        // Decode the plain byte array and sanity check
        let prev_hash_stratum_order = hex_decode(value).context("Parsing hex bytes failed")?;
        if prev_hash_stratum_order.len() != 32 {
            return Err(ErrorKind::Json(format!(
                "Incorrect prev hash length: {}",
                prev_hash_stratum_order.len()
            ))
            .into());
        }
        // Swap every u32 from big endian to little endian byte order
        for chunk in prev_hash_stratum_order.chunks(size_of::<u32>()) {
            let prev_hash_word = BigEndian::read_u32(chunk);
            prev_hash_cursor
                .write_u32::<LittleEndian>(prev_hash_word)
                .expect("Internal error: Could not write buffer");
        }

        Ok(PrevHash(prev_hash_cursor.into_inner()))
    }
}

/// TODO: this is not the cleanest way as any deserialization error is essentially consumed and
/// manifested as empty vector. However, it is very comfortable to use this trait implementation
/// in Extranonce1 serde support
impl From<String> for PrevHash {
    fn from(value: String) -> Self {
        PrevHash::try_from(value.as_str()).unwrap_or(PrevHash(vec![]))
    }
}

/// Helper Serializer that peforms the reverse process of converting the prev hash into stratum V1
/// ordering
/// TODO: implement unit test
impl Into<String> for PrevHash {
    fn into(self) -> String {
        let mut prev_hash_stratum_cursor = std::io::Cursor::new(Vec::new());
        // swap every u32 from little endian to big endian
        for chunk in self.0.chunks(size_of::<u32>()) {
            let prev_hash_word = LittleEndian::read_u32(chunk);
            prev_hash_stratum_cursor
                .write_u32::<BigEndian>(prev_hash_word)
                .expect("Internal error: Could not write buffer");
        }
        hex::encode(prev_hash_stratum_cursor.into_inner())
    }
}

/// Little-endian hex encoded u32
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(into = "String", from = "String")]
pub struct HexU32Le(pub u32);

impl TryFrom<&str> for HexU32Le {
    type Error = crate::error::Error;

    fn try_from(value: &str) -> Result<Self> {
        let parsed_bytes: [u8; 4] = FromHex::from_hex(value).context("parse u32 hex value")?;
        Ok(HexU32Le(u32::from_le_bytes(parsed_bytes)))
    }
}

/// TODO: this is not the cleanest way as any deserialization error is essentially consumed and
/// manifested as empty vector. However, it is very comfortable to use this trait implementation
/// in Extranonce1 serde support
impl From<String> for HexU32Le {
    fn from(value: String) -> Self {
        HexU32Le::try_from(value.as_str()).unwrap_or(HexU32Le(0))
    }
}

/// Helper Serializer
impl Into<String> for HexU32Le {
    fn into(self) -> String {
        self.0.to_le_bytes().to_hex()
    }
}

/// Big-endian alternative of the HexU32
/// TODO: find out how to consolidate/parametrize it with generic parameters
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(into = "String", from = "String")]
pub struct HexU32Be(pub u32);

impl TryFrom<&str> for HexU32Be {
    type Error = crate::error::Error;

    fn try_from(value: &str) -> Result<Self> {
        let parsed_bytes: [u8; 4] = FromHex::from_hex(value).context("parse u32 hex value")?;
        Ok(HexU32Be(u32::from_be_bytes(parsed_bytes)))
    }
}

/// TODO: this is not the cleanest way as any deserialization error is essentially consumed and
/// manifested as empty vector. However, it is very comfortable to use this trait implementation
/// in Extranonce1 serde support
impl From<String> for HexU32Be {
    fn from(value: String) -> Self {
        HexU32Be::try_from(value.as_str()).unwrap_or(HexU32Be(0))
    }
}

/// Helper Serializer
impl Into<String> for HexU32Be {
    fn into(self) -> String {
        self.0.to_be_bytes().to_hex()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::v1::*;
    use bytes::BytesMut;

    use ii_async_compat::{bytes, tokio};

    /// Test traits that will be used by serded for HexBytes when converting from/to string
    #[test]
    fn test_hex_bytes() {
        let hex_bytes = HexBytes(vec![0xde, 0xad, 0xbe, 0xef, 0x11, 0x22, 0x33]);
        let hex_bytes_str = "deadbeef112233";

        let checked_hex_bytes_str: String = hex_bytes.clone().into();
        assert_eq!(
            hex_bytes_str, checked_hex_bytes_str,
            "Mismatched hex bytes strings",
        );

        let checked_hex_bytes = HexBytes::try_from(hex_bytes_str).expect("");

        assert_eq!(hex_bytes, checked_hex_bytes, "Mismatched hex bytes values",)
    }

    #[test]
    fn test_extra_nonce1() {
        let expected_enonce1 =
            ExtraNonce1(HexBytes(vec![0xde, 0xad, 0xbe, 0xef, 0x11, 0x22, 0x33]));
        let expected_enonce1_str = r#""deadbeef112233""#;

        let checked_enonce1_str: String =
            serde_json::to_string(&expected_enonce1).expect("Serialization failed");
        assert_eq!(
            expected_enonce1_str, checked_enonce1_str,
            "Mismatched extranonce 1 strings",
        );

        let checked_enonce1 =
            serde_json::from_str(expected_enonce1_str).expect("Deserialization failed");

        assert_eq!(
            expected_enonce1, checked_enonce1,
            "Mismatched extranonce 1 values",
        )
    }

    /// This test demonstrates an actual implementation of protocol handler for a set of
    /// messsages
    #[tokio::test]
    async fn test_build_message_from_frame() {
        for &req in V1_TEST_REQUESTS {
            let msg = build_message_from_frame(Frame::from_serialized_payload(BytesMut::from(req)))
                .expect("Deserialization failed");
            msg.accept(&mut TestIdentityHandler).await;
        }
    }

    #[tokio::test]
    async fn test_deserialize_response_message() {
        let _msg = build_message_from_frame(Frame::from_serialized_payload(BytesMut::from(
            MINING_SUBSCRIBE_OK_RESULT_JSON,
        )))
        .expect("Deserialization failed");
    }

    // add also a separate stratum error test as per above response
}
