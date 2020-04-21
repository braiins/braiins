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

//! This module defines framing of Stratum V1 messages

pub mod codec;

use bytes::{buf::BufMutExt, BytesMut};

use ii_async_compat::bytes;

use super::Protocol;
use crate::error::{Error, Result};
use crate::payload::{Payload, SerializablePayload};

/// Protocol frame consists solely from the payload
#[derive(Debug, PartialEq)]
pub struct Frame(Payload<Protocol>);

impl Frame {
    /// TODO use this constant
    pub const MAX_FRAME_LENGTH: usize = 16384;

    /// Builds a frame from `src`. No copying occurs as `BytesMut` allows us splitting off
    /// the payload part.
    pub(crate) fn deserialize(src: &mut BytesMut) -> Result<Self> {
        Ok(Self::from_serialized_payload(src.split()))
    }

    pub fn from_serialized_payload(payload: BytesMut) -> Self {
        Self(Payload::from(payload))
    }

    pub fn from_serializable_payload<T>(payload: T) -> Self
    // TODO review the static lifetime
    where
        T: 'static + SerializablePayload<Protocol>,
    {
        Self(Payload::LazyBytes(Box::new(payload)))
    }

    /// Serializes a frame into a specified `dst` buffer. The method either copies the already
    /// serialized payload into the buffer or runs the on-demand serializer of the payload.
    pub(crate) fn serialize(&self, dst: &mut BytesMut) -> Result<()> {
        // TODO reserve a reasonable chunk in the buffer - make it a constant
        dst.reserve(128);
        let mut payload_writer = dst.split().writer();

        self.0.serialize_to_writer(&mut payload_writer)?;

        // No copying occurs here as the buffer has been originally split off of `dst`
        dst.unsplit(payload_writer.into_inner());

        Ok(())
    }

    /// Consumes the frame providing its payload
    pub fn into_inner(self) -> Payload<Protocol> {
        self.0
    }
}

#[cfg(test)]
mod test {
    use super::super::{Handler, MessageId};
    use super::*;
    use async_trait::async_trait;

    #[test]
    fn test_frame_from_serializable_payload() {
        const EXPECTED_FRAME_BYTES: &'static [u8] = &[0xde, 0xad, 0xbe, 0xef, 0x0a];
        struct TestPayload;

        #[async_trait]
        impl SerializablePayload<Protocol> for TestPayload {
            async fn accept(&self, _id: &MessageId, _handler: &mut dyn Handler) {
                panic!("BUG: no handling for TestPayload");
            }

            fn serialize_to_writer(&self, writer: &mut dyn std::io::Write) -> Result<()> {
                writer.write(&EXPECTED_FRAME_BYTES)?;
                Ok(())
            }
        }
        let frame = Frame::from_serializable_payload(TestPayload);

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

#[derive(Debug)]
pub struct Framing;

impl ii_wire::Framing for Framing {
    type Tx = Frame;
    type Rx = Frame;
    type Error = Error;
    type Codec = codec::Codec;
}
