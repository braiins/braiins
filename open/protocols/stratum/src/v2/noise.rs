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

//! Noise protocol implementation for Stratum V2. This module provides helper objects that process
//! the selected handshake pattern on initiator as well as on responder and eventually provide a
//! TransportState of the noise, that will be used for running the AEAD communnication.

use bytes::{Bytes, BytesMut};
use snow::{params::NoiseParams, Builder, HandshakeState, TransportState};
use std::convert::TryFrom;
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, FramedParts};

use ii_async_compat::prelude::*;
use ii_wire;

use crate::error::{Error, ErrorKind, Result, ResultExt};
use crate::v2;

pub mod codec;
pub use codec::Codec;

pub mod auth;
mod handshake;

/// Static keypair (aka 's' and 'rs') from the noise handshake patterns. This has to be used by
/// users of this noise when Building the responder
pub use snow::Keypair as StaticKeypair;
/// Snow doesn't have a dedicated public key type, we will need it for authentication
pub type StaticPublicKey = Vec<u8>;
/// Snow doesn't have a dedicated secret key type, we will need it for authentication
pub type StaticSecretKey = Vec<u8>;
/// Public key of the authority that the initiator uses to authenticate the remote server by
/// verifying certificate signature
pub use ed25519_dalek::PublicKey as AuthorityPublicKey;

const PARAMS: &'static str = "Noise_NX_25519_ChaChaPoly_BLAKE2s";
// TODO: the following constants are public in snow but the constants module itself is private.
//  Submit patch to snow fixing it.
pub const MAX_MESSAGE_SIZE: usize = 65535;
pub const TAGLEN: usize = 16;
pub const MAX_PAYLOAD_SIZE: usize = MAX_MESSAGE_SIZE - TAGLEN;

/// Special framing for noise messages, Helper struct that groups all framing related associated
/// types (Frame + Error + Codec) for the `ii_wire::Framing` trait
#[derive(Debug)]
pub struct Framing;

impl ii_wire::Framing for Framing {
    type Tx = BytesMut;
    type Rx = BytesMut;
    type Error = Error;
    type Codec = codec::Codec;
}

/// Tcp stream that produces/consumes noise frames
type NoiseFramedTcpStream = Framed<TcpStream, <Framing as ii_wire::Framing>::Codec>;

/// Generates noise specific static keypair specific for the current params
pub fn generate_keypair() -> Result<StaticKeypair> {
    let params: NoiseParams = PARAMS.parse().expect("BUG: cannot parse noise parameters");
    let builder: Builder<'_> = Builder::new(params);
    builder.generate_keypair().map_err(Into::into)
}

#[derive(Debug)]
pub struct Initiator {
    stage: usize,
    handshake_state: HandshakeState,
    /// Public key that the Initiatior will use to construct a 'Certificate' on the fly from
    /// the SignatureNoiseMessage and of the static public key of the `Responder` and will verify
    /// the authenticity of the static public key of the Responder
    authority_public_key: ed25519_dalek::PublicKey,
}

impl Initiator {
    pub fn new(authority_public_key: ed25519_dalek::PublicKey) -> Self {
        let params: NoiseParams = PARAMS.parse().expect("BUG: cannot parse noise parameters");

        // Initialize our initiator using a builder.
        let builder: Builder<'_> = Builder::new(params);
        let handshake_state = builder
            .build_initiator()
            .expect("BUG: cannot build initiator");

        Self {
            stage: 0,
            handshake_state,
            authority_public_key,
        }
    }

    pub async fn connect(self, connection: TcpStream) -> Result<v2::Framed> {
        let mut noise_framed_stream = ii_wire::Connection::<Framing>::new(connection).into_inner();

        let handshake = handshake::Handshake::new(self);
        let transport_mode = handshake.run(&mut noise_framed_stream).await?;

        Ok(transport_mode.into_stratum_framed_stream(noise_framed_stream))
    }

    /// Verify the signature of the remote static key
    /// TODO: verify the signature of the remote static public key based on:
    ///  - remote central authority public key (must be provided to Initiator instance upon
    ///    creation)
    fn verify_remote_static_key_signature(
        &mut self,
        signature_noise_message: BytesMut,
    ) -> Result<()> {
        let remote_static_key = self
            .handshake_state
            .get_remote_static()
            .expect("BUG: remote static has not been provided yet");
        let remote_static_key = StaticPublicKey::from(remote_static_key);
        let signature_noise_message =
            auth::SignatureNoiseMessage::try_from(&signature_noise_message[..])?;

        let certificate = auth::Certificate::from_noise_message(
            signature_noise_message,
            remote_static_key,
            self.authority_public_key,
        );
        certificate
            .validate()
            .context("Validation of certificate")?;

        Ok(())
    }
}

impl handshake::Step for Initiator {
    fn into_handshake_state(self) -> HandshakeState {
        self.handshake_state
    }

    fn step(
        &mut self,
        in_msg: Option<handshake::Message>,
        mut noise_bytes: BytesMut,
    ) -> Result<handshake::StepResult> {
        let mut buf = vec![0u8; MAX_MESSAGE_SIZE];
        let result = match self.stage {
            0 => {
                // -> e
                let len_written = self.handshake_state.write_message(&[], &mut buf)?;
                noise_bytes.extend_from_slice(&buf[..len_written]);
                handshake::StepResult::ExpectReply(handshake::Message::new(noise_bytes))
            }
            1 => {
                // <- e, ee, s, es
                let in_msg = in_msg.ok_or(ErrorKind::Noise("No message arrived".to_string()))?;
                let signature_len = self.handshake_state.read_message(&in_msg.inner, &mut buf)?;
                self.verify_remote_static_key_signature(BytesMut::from(&buf[..signature_len]))
                    .context("Certificate signature verification")?;
                handshake::StepResult::Done
            }
            _ => {
                panic!("BUG: No more steps that can be done by the Initiator in Noise handshake");
            }
        };
        self.stage += 1;
        Ok(result)
    }
}

pub struct Responder {
    stage: usize,
    handshake_state: HandshakeState,
    /// Serialized signature noise message that can be directly provided as part of the
    /// handshake - see `step()`
    signature_noise_message: Bytes,
}

impl Responder {
    /// TODO add static keypair signature and store it inside the instance
    pub fn new(static_keypair: &StaticKeypair, signature_noise_message: Bytes) -> Self {
        let params: NoiseParams = PARAMS.parse().expect("BUG: cannot parse noise parameters");

        // Initialize our initiator using a builder.
        let builder: Builder<'_> = Builder::new(params);
        let handshake_state = builder
            .local_private_key(&static_keypair.private)
            .build_responder()
            .expect("BUG: cannot build responder");

        Self {
            stage: 0,
            handshake_state,
            signature_noise_message,
        }
    }

    pub async fn accept(self, connection: TcpStream) -> Result<v2::Framed> {
        // Run the handshake and switch to transport mode
        let mut noise_framed_stream = ii_wire::Connection::<Framing>::new(connection).into_inner();

        let handshake = handshake::Handshake::new(self);
        let transport_mode = handshake.run(&mut noise_framed_stream).await?;

        Ok(transport_mode.into_stratum_framed_stream(noise_framed_stream))
    }
}

impl handshake::Step for Responder {
    fn into_handshake_state(self) -> HandshakeState {
        self.handshake_state
    }

    fn step(
        &mut self,
        in_msg: Option<handshake::Message>,
        mut noise_bytes: BytesMut,
    ) -> Result<handshake::StepResult> {
        let mut buf = vec![0u8; MAX_MESSAGE_SIZE];

        let result = match self.stage {
            0 => handshake::StepResult::ReceiveMessage,
            1 => {
                // <- e
                let in_msg = in_msg.ok_or(ErrorKind::Noise("No message arrived".to_string()))?;
                self.handshake_state.read_message(&in_msg.inner, &mut buf)?;
                // Send the signature along this message
                // TODO: use actual signature stored inside the responder instance
                // -> e, ee, s, es [encrypted signature]
                let len_written = self
                    .handshake_state
                    .write_message(&self.signature_noise_message, &mut buf)?;
                noise_bytes.extend_from_slice(&buf[..len_written]);
                handshake::StepResult::NoMoreReply(handshake::Message::new(noise_bytes))
            }
            2 => handshake::StepResult::Done,
            _ => {
                panic!("BUG: No more steps that can be done by the Initiator in Noise handshake");
            }
        };
        self.stage += 1;
        Ok(result)
    }
}

/// Helper struct that wraps the transport state and provides convenient interface to read/write
/// messages
#[derive(Debug)]
pub struct TransportMode {
    inner: TransportState,
}

impl TransportMode {
    pub fn new(inner: TransportState) -> Self {
        Self { inner }
    }

    /// Consumes the transport mode instance and converts it into a Framed stream that can
    /// consume/produce v2 frames with noise encryption
    pub fn into_stratum_framed_stream(
        self,
        noise_framed_stream: NoiseFramedTcpStream,
    ) -> v2::Framed {
        // Take apart the noise framed stream and build a new Framed stream that  uses
        // stratum V2 framing codec composed with the noise codec (in transport mode)
        let mut noise_framed_parts = noise_framed_stream.into_parts();

        // Move the noise codec into transport mode
        noise_framed_parts.codec.set_transport_mode(self);
        let v2_codec =
            <v2::framing::Framing as ii_wire::Framing>::Codec::new(Some(noise_framed_parts.codec));

        let mut v2_framed_parts = FramedParts::new(noise_framed_parts.io, v2_codec);
        v2_framed_parts
            .read_buf
            .unsplit(noise_framed_parts.read_buf);
        v2_framed_parts
            .write_buf
            .unsplit(noise_framed_parts.write_buf);

        Framed::from_parts(v2_framed_parts)
    }

    /// Decrypt and verify message from `in_buf` and append the result to `decrypted_message`
    /// It is an adaptor for not a very convenient interface of Snow that requires fixed size
    /// buffers
    pub fn read(&mut self, encrypted_msg: BytesMut, decrypted_msg: &mut BytesMut) -> Result<()> {
        let mut out_vec = vec![0u8; MAX_MESSAGE_SIZE];
        let msg_len = self.inner.read_message(&encrypted_msg[..], &mut out_vec)?;
        decrypted_msg.extend_from_slice(&out_vec[..msg_len]);

        Ok(())
    }

    /// Encrypt a message specified in `plan_msg` and write the encrypted message into a specified
    /// `encrypted_msg` buffer.
    /// It is an adaptor for not a very convenient interface of Snow that requires fixed size
    /// buffers
    pub fn write(&mut self, plain_msg: BytesMut, encrypted_msg: &mut BytesMut) -> Result<()> {
        let mut out_vec = vec![0u8; MAX_MESSAGE_SIZE];
        let msg_len = self.inner.write_message(&plain_msg[..], &mut out_vec)?;
        encrypted_msg.extend_from_slice(&out_vec[..msg_len]);

        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use handshake::Step as _;

    /// Helper that builds:
    /// - serialized signature noise message
    /// - certification authority key pair
    /// - server (responder) static key pair
    fn build_serialized_signature_noise_message_and_keypairs(
    ) -> (Bytes, ed25519_dalek::Keypair, StaticKeypair) {
        let (signed_part, authority_keypair, static_keypair, signature) =
            auth::test::build_test_signed_part_and_auth();
        let certificate = auth::Certificate::new(signed_part, signature);
        let signature_noise_message = certificate
            .build_noise_message()
            .serialize_to_bytes_mut()
            .expect("BUG: Cannot serialize signature noise message")
            .freeze();
        (signature_noise_message, authority_keypair, static_keypair)
    }

    pub(crate) fn perform_handshake() -> (TransportMode, TransportMode) {
        // Prepare test certificate and a serialized noise message that contains the signature
        let (signature_noise_message, authority_keypair, static_keypair) =
            build_serialized_signature_noise_message_and_keypairs();

        let mut initiator = Initiator::new(authority_keypair.public);

        let mut responder = Responder::new(&static_keypair, signature_noise_message);
        let mut initiator_in_msg: Option<handshake::Message> = None;

        // Verify that responder expects to receive the first message
        assert_eq!(
            responder
                .step(None, BytesMut::new())
                .expect("BUG: responder failed in the first step"),
            handshake::StepResult::ReceiveMessage
        );

        loop {
            let initiator_buf: BytesMut = BytesMut::new();
            let responder_buf: BytesMut = BytesMut::new();

            match initiator
                .step(initiator_in_msg.clone(), initiator_buf)
                .expect("BUG: Initiator failed")
            {
                handshake::StepResult::ReceiveMessage => {
                    panic!("BUG: Initiator must not request the first message!");
                }
                handshake::StepResult::ExpectReply(initiator_out_msg) => {
                    match responder
                        .step(Some(initiator_out_msg), responder_buf)
                        .expect("BUG: responder failed")
                    {
                        handshake::StepResult::ExpectReply(responder_out_msg)
                        | handshake::StepResult::NoMoreReply(responder_out_msg) => {
                            (&mut initiator_in_msg).replace(responder_out_msg);
                        }
                        handshake::StepResult::Done | handshake::StepResult::ReceiveMessage => {
                            panic!("BUG: Responder didn't yield any response!");
                        }
                    }
                }
                handshake::StepResult::NoMoreReply(initiator_out_msg) => {
                    match responder
                        .step(Some(initiator_out_msg), responder_buf)
                        .expect("BUG: responder failed")
                    {
                        handshake::StepResult::ExpectReply(responder_out_msg)
                        | handshake::StepResult::NoMoreReply(responder_out_msg) => panic!(
                            "BUG: Responder provided an unexpected response {:?}",
                            responder_out_msg
                        ),
                        // Responder is now either done or may request another message
                        handshake::StepResult::ReceiveMessage | handshake::StepResult::Done => {}
                    }
                }
                // Initiator is now finalized
                handshake::StepResult::Done => break,
            };
        }
        let initiator_transport_mode = TransportMode::new(
            initiator
                .into_handshake_state()
                .into_transport_mode()
                .expect("BUG: cannot convert initiator into transport mode"),
        );
        let responder_transport_mode = TransportMode::new(
            responder
                .into_handshake_state()
                .into_transport_mode()
                .expect("BUG: cannot convert responder into transport mode"),
        );

        (initiator_transport_mode, responder_transport_mode)
    }
    /// Verifies that initiator and responder can successfully perform a handshake
    #[test]
    fn test_handshake() {
        let (mut initiator_transport_mode, mut responder_transport_mode) = perform_handshake();

        // Verify we can send/receive messages between initiator and responder
        let message = b"test message";
        let mut encrypted_msg = BytesMut::new();
        let mut decrypted_msg = BytesMut::new();

        initiator_transport_mode
            .write(BytesMut::from(&message[..]), &mut encrypted_msg)
            .expect("BUG: initiator failed to write message");

        responder_transport_mode
            .read(encrypted_msg, &mut decrypted_msg)
            .expect("BUG: responder failed to read transport message");
        assert_eq!(&message[..], &decrypted_msg, "Messages don't match");
    }

    fn bind_test_server() -> Option<(ii_wire::Server, ii_wire::Address)> {
        const ADDR: &'static str = "127.0.0.1";
        const MIN_PORT: u16 = 9999;
        const MAX_PORT: u16 = 10001;

        // Find first available port for the test
        for port in MIN_PORT..MAX_PORT {
            let addr = ii_wire::Address(ADDR.into(), port);
            if let Ok(server) = ii_wire::Server::bind(&addr) {
                return Some((server, addr));
            }
        }
        None
    }

    #[tokio::test]
    async fn test_initiator_connect_responder_accept() {
        let (mut server, addr) =
            bind_test_server().expect("BUG: binding failed, no available ports");
        let payload = BytesMut::from(&[1u8, 2, 3, 4][..]);
        let expected_frame =
            v2::framing::Frame::from_serialized_payload(true, 0, 0x16, payload.clone());
        // This is currently due to the fact that Frame doesn't support cloning and it will be
        // consumed by the initiator codec
        let expected_frame_copy =
            v2::framing::Frame::from_serialized_payload(true, 0, 0x16, payload);

        // Prepare test certificate and a serialized noise message that contains the signature
        let (signature_noise_message, authority_keypair, static_keypair) =
            build_serialized_signature_noise_message_and_keypairs();

        // Spawn server task that reacts to any incoming message and responds
        // with SetupConnectionSuccess
        tokio::spawn(async move {
            let responder = Responder::new(&static_keypair, signature_noise_message);

            let conn = server
                .next()
                .await
                .expect("BUG: Server has terminated")
                .expect("BUG: Server returned an error");

            let mut server_framed_stream = responder
                .accept(conn)
                .await
                .expect("BUG: Responder: noise handshake failed");

            server_framed_stream
                .send(expected_frame)
                .await
                .expect("BUG: Cannot send a stream")
        });

        let mut client = ii_wire::Client::new(addr);
        let connection = client
            .next()
            .await
            .expect("BUG: Cannot connect to noise endpoint");

        let initiator = Initiator::new(authority_keypair.public);
        let mut client_framed_stream = initiator
            .connect(connection)
            .await
            .expect("BUG: cannot connect to noise responder");

        let received_frame = client_framed_stream
            .next()
            .await
            .expect("BUG: connection unexpectedly terminated")
            .expect("BUG: error when receiving stream");
        assert_eq!(
            expected_frame_copy, received_frame,
            "BUG: Expected ({:x?}) and decoded ({:x?}) frames don't match",
            expected_frame_copy, received_frame
        );
    }
}
