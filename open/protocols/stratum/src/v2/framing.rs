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

//! This module defines basic framing and all protocol message types

use bytes::{
    buf::{Buf, BufMut, BufMutExt},
    BytesMut,
};

use ii_async_compat::bytes;
use ii_logging::macros::*;

use super::Protocol;
use crate::error::{Error, Result};
use crate::payload::{Payload, SerializablePayload};

pub mod codec;

/// Message type field in the frame header
pub type MsgType = u8;
/// Extension type field in the frame header
pub type ExtType = u16;

/// Header of each stratum protocol frame
/// This object has custom serialization and deserialization
#[derive(Debug, PartialEq, Clone)]
pub struct Header {
    /// Indicates whether payload contains a channel message
    pub is_channel_message: bool,
    /// Unique identifier of the extension describing this protocol message
    pub extension_type: ExtType,
    /// Unique identifier of the message within the extension_type namespace
    pub msg_type: MsgType,
    /// Length of the protocol message, not including this header
    pub msg_length: Option<u32>,
}

impl Header {
    pub const SIZE: usize = 6;
    pub const LEN_OFFSET: usize = 3;
    pub const LEN_SIZE: usize = 3;
    pub const MAX_LEN: u32 = 0xffffff;
    /// Bit position of the 'is_channel_message' flag
    const CHANNEL_MSG_SHIFT: usize = 15;
    const CHANNEL_MSG_MASK: u16 = 1u16 << Self::CHANNEL_MSG_SHIFT;

    pub fn new(
        is_channel_message: bool,
        extension_type: ExtType,
        msg_type: MsgType,
        msg_length: Option<u32>,
    ) -> Self {
        assert!(
            msg_length.unwrap_or(0) <= Self::MAX_LEN,
            "BUG: Message too large, request: {} bytes, max allowed {} bytes",
            msg_length.unwrap(),
            Self::MAX_LEN
        );
        Self {
            is_channel_message,
            extension_type: extension_type.into(),
            msg_type,
            msg_length,
        }
    }

    /// Serializes the header into the specified `dst` buffer and fills out the length field
    /// based on user specified optional value in `msg_length` or takes the value already set in
    /// the header
    pub fn serialize(&self, dst: &mut BytesMut, msg_length: Option<u32>) {
        // Determine the message length
        let msg_length = msg_length.or(self.msg_length).expect(
            "BUG: no message length specified for serialization nor the header already has a \
             predefined message length!",
        );

        let extension_field: ExtType =
            self.extension_type | (u16::from(self.is_channel_message) << Self::CHANNEL_MSG_SHIFT);
        dst.put_u16_le(extension_field);
        dst.put_u8(self.msg_type);
        dst.put_uint_le(msg_length as u64, Self::LEN_SIZE);
    }

    /// Deserializes a `Header` from `src`
    pub fn deserialize(src: &mut BytesMut) -> Self {
        let extension_field = src.get_u16_le();
        let msg_type = src.get_u8();
        let length = src.get_uint_le(Self::LEN_SIZE) as u32;

        Self {
            is_channel_message: (extension_field & Self::CHANNEL_MSG_MASK) != 0,
            extension_type: extension_field & !Self::CHANNEL_MSG_MASK,
            msg_type,
            msg_length: Some(length),
        }
    }
}

/// Protocol frame
/// The frame provides `header` information for payload dispatching etc. Direct access to payload
/// is not possible as it requires complex handling. For payload processing it is recommended to use
/// `Frame::split()`
#[derive(Debug, PartialEq)]
pub struct Frame {
    /// Allow public access to the header for payload dispatching etc.
    pub header: Header,
    /// Keep payload
    pub(crate) payload: Payload<Protocol>,
}

impl Frame {
    /// Builds a frame from `src`. No copying occurs as `BytesMut` allows us splitting off
    /// the payload part. The method panics if  `src` doesn't contain exactly one frame.
    fn deserialize(src: &mut BytesMut) -> Result<Self> {
        let header = Header::deserialize(src);
        // Missing length is considered a bug
        let msg_len: u32 = header.msg_length.expect("BUG: missing header length field");
        // The caller (possibly decoder) should have ensured correct framing. It is only a sanity
        // check that the expected length and remaining bytes in the buffer.
        // Note, that header deserialization also took the relevant part of the
        // source bytes, therefore its remaining size is reduced.
        assert_eq!(
            msg_len as usize,
            src.len(),
            "BUG: malformed message header ({:x?}) - message length ({}) and remaining data length \
            ({}) don't match",
            header, msg_len, src.len()
        );
        trace!("V2: deserialized header: {:?}", header);
        let payload = src.split();
        Ok(Self {
            header,
            payload: payload.into(),
        })
    }

    pub fn from_serialized_payload(
        is_channel_msg: bool,
        ext_type: ExtType,
        msg_type: MsgType,
        payload: BytesMut,
    ) -> Self {
        let header = Header::new(
            is_channel_msg,
            ext_type,
            msg_type,
            Some(payload.len() as u32),
        );
        Self {
            header,
            payload: payload.into(),
        }
    }

    pub fn from_serializable_payload<T>(
        is_channel_msg: bool,
        ext_type: ExtType,
        msg_type: MsgType,
        payload: T,
    ) -> Self
    // TODO review the static lifetime
    where
        T: 'static + SerializablePayload<Protocol>,
    {
        let header = Header::new(is_channel_msg, ext_type, msg_type, None);
        Self {
            header,
            payload: Payload::from_serializable(payload),
        }
    }

    /// Serializes a frame into a specified `dst` buffer. The method either copies the already
    /// serialized payload into the buffer or runs the on-demand serializer of the payload.
    fn serialize(&self, dst: &mut BytesMut) -> Result<()> {
        // TODO reserve a reasonable chunk in the buffer - make it a constant
        dst.reserve(128);
        let mut payload_writer = dst.split_off(Header::SIZE).writer();
        self.payload.serialize_to_writer(&mut payload_writer)?;
        // Writer not needed anymore, the underlying BytesMut now contains the serialized payload
        let payload_buf = payload_writer.into_inner();
        // Serialize the header since now can determine the actual payload length
        let payload_len = payload_buf.len();
        self.header.serialize(dst, Some(payload_len as u32));

        // Join the payload with the header (no copying occurs here as the buffer has been
        // originally split off of `dst`
        dst.unsplit(payload_buf);

        Ok(())
    }

    /// Consumes the frame providing its header and payload
    pub fn split(self) -> (Header, Payload<Protocol>) {
        (self.header, self.payload)
    }
}

/// Helper struct that groups all framing related associated types (Frame + Error +
/// Codec) for the `ii_wire::Framing` trait
#[derive(Debug)]
pub struct Framing;

impl ii_wire::Framing for Framing {
    type Tx = Frame;
    type Rx = Frame;
    type Error = Error;
    type Codec = codec::Codec;
}

pub const PAYLOAD_CHANNEL_OFFSET: usize = 4;

#[cfg(test)]
mod test {
    use super::*;
    use async_trait::async_trait;

    #[test]
    fn test_header_serialization() {
        let expected_bytes = BytesMut::from(&[0x00u8, 0x80, 0x16, 0xcc, 0xbb, 0xaa][..]);

        let header = Header::deserialize(&mut expected_bytes.clone());

        let mut header_bytes = BytesMut::new();
        header.serialize(&mut header_bytes, Some(0xaabbcc as u32));
        assert_eq!(
            BytesMut::from(&expected_bytes[..]),
            header_bytes,
            "Serialized header {:x?} doesn't match expected: {:x?}, received: {:x?}",
            header,
            expected_bytes,
            header_bytes
        );
    }

    /// Verify invalid header with empty length field panics upon serialization if we don't
    /// specify the length field explicitely during serialization
    #[test]
    #[should_panic]
    fn test_header_serialization_panic() {
        let header = Header::new(true, 0, 0x16, None);

        let mut header_bytes = BytesMut::new();
        header.serialize(&mut header_bytes, None);
    }

    #[test]
    #[should_panic]
    fn test_header_too_large_message() {
        let _header = Header::new(true, 0, 0x16, Some(Header::MAX_LEN + 1));
    }

    #[test]
    fn test_frame_deserialize() {
        let frame_bytes = [0x00u8, 0x80, 0x16, 0x04, 0x00, 0x00, 0xde, 0xad, 0xbe, 0xef];
        let mut frame_bytes_buf = BytesMut::new();
        frame_bytes_buf.extend_from_slice(&frame_bytes);

        let mut expected_payload = BytesMut::new();
        expected_payload.extend_from_slice(&frame_bytes[frame_bytes.len() - 4..]);
        let expected_frame = Frame::from_serialized_payload(true, 0, 0x16, expected_payload);

        let frame = Frame::deserialize(&mut frame_bytes_buf).expect("Building frame failed");

        assert_eq!(expected_frame, frame, "Frames don't match");
    }

    pub(crate) fn build_large_payload(length: usize) -> BytesMut {
        const CHUNK_SIZE: usize = 256;
        let chunk = [0xaa_u8; CHUNK_SIZE];
        let mut payload = BytesMut::with_capacity(length);
        for _i in 0..length / CHUNK_SIZE {
            payload.extend_from_slice(&chunk[..])
        }
        // Append the last chunk
        payload.extend_from_slice(&chunk[..length - payload.len()]);

        payload
    }

    #[test]
    fn test_large_frame() {
        let mut frame_bytes_buf = BytesMut::new();

        let payload = build_large_payload(Header::MAX_LEN as usize);
        let expected_frame =
            Frame::from_serialized_payload(true, 0, 0x16, BytesMut::from(&payload[..]));
        expected_frame
            .serialize(&mut frame_bytes_buf)
            .expect("Expected frame serialization failed");

        let frame = Frame::deserialize(&mut frame_bytes_buf).expect("Cannot deserialize frame");

        assert_eq!(expected_frame, frame, "Frames don't match");
    }

    #[test]
    #[should_panic]
    fn test_too_large_frame() {
        let payload = build_large_payload(Header::MAX_LEN as usize + 1);
        let _expected_frame =
            Frame::from_serialized_payload(true, 0, 0x16, BytesMut::from(&payload[..]));
    }

    #[test]
    fn test_frame_from_serializable_payload() {
        const EXPECTED_FRAME_BYTES: &'static [u8] =
            &[0x00u8, 0x80, 0x16, 0x04, 0x00, 0x00, 0xde, 0xad, 0xbe, 0xef];

        struct TestPayload;

        #[async_trait]
        impl SerializablePayload<Protocol> for TestPayload {
            async fn accept(
                &self,
                _header: &<Protocol as crate::Protocol>::Header,
                _handler: &mut <Protocol as crate::Protocol>::Handler,
            ) {
                panic!("BUG: no handling for TestPayload");
            }

            fn serialize_to_writer(&self, writer: &mut dyn std::io::Write) -> Result<()> {
                writer.write(&EXPECTED_FRAME_BYTES[6..])?;
                Ok(())
            }
        }
        let frame = Frame::from_serializable_payload(true, 0, 0x16, TestPayload);

        let mut dst_frame_bytes = BytesMut::new();
        assert!(frame.serialize(&mut dst_frame_bytes).is_ok());
        assert_eq!(
            BytesMut::from(EXPECTED_FRAME_BYTES),
            dst_frame_bytes,
            "Frames don't match, expected: {:x?}, generated: {:x?}",
            EXPECTED_FRAME_BYTES,
            // convert to vec as it has same Debug representation
            dst_frame_bytes.to_vec()
        );
    }
}
