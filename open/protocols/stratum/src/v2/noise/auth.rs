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

//! Authentication module that provides pubkey and certificate handling API

use bytes::{buf::BufMutExt, BytesMut};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::time::{Duration, SystemTime};

use ii_async_compat::bytes;

use crate::error::{Error, Result};
use crate::v2::{self, noise::StaticPublicKey};

mod formats;
pub use formats::*;

/// Header of the `SignedPart` that will also be part of the `Certificate`
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct SignedPartHeader {
    version: u16,
    // Validity start time (unix timestamp)
    valid_from: u32,
    // Signature is invalid after this point in time (unix timestamp)
    not_valid_after: u32,
}

impl SignedPartHeader {
    const VERSION: u16 = 0;

    pub fn with_duration(valid_for: Duration) -> Result<Self> {
        let valid_from = SystemTime::now();
        let not_valid_after = valid_from + valid_for;
        Ok(Self {
            version: Self::VERSION,
            valid_from: Self::system_time_to_unix_time_u32(&valid_from)?,
            not_valid_after: Self::system_time_to_unix_time_u32(&not_valid_after)?,
        })
    }

    pub fn valid_from(&self) -> SystemTime {
        Self::unix_time_u32_to_system_time(self.valid_from)
            .expect("BUG: cannot provide 'valid_from' time")
    }

    pub fn not_valid_after(&self) -> SystemTime {
        Self::unix_time_u32_to_system_time(self.not_valid_after)
            .expect("BUG: cannot provide 'not_valid_after' time")
    }

    pub fn verify_expiration(&self, now: SystemTime) -> Result<()> {
        let now_timestamp = Self::system_time_to_unix_time_u32(&now)?;
        if now_timestamp < self.valid_from {
            return Err(Error::Noise(format!(
                "Certificate not yet valid, valid from: {:?}, now: {:?}",
                self.valid_from, now
            ))
            .into());
        }
        if now_timestamp > self.not_valid_after {
            return Err(Error::Noise(format!(
                "Certificate expired, not valid after: {:?}, now: {:?}",
                self.valid_from, now
            ))
            .into());
        }
        Ok(())
    }

    fn system_time_to_unix_time_u32(t: &SystemTime) -> Result<u32> {
        t.duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as u32)
            .map_err(|e| {
                Error::Noise(format!(
                    "Cannot convert system time to unix timestamp: {}",
                    e
                ))
                .into()
            })
    }

    fn unix_time_u32_to_system_time(unix_timestamp: u32) -> Result<SystemTime> {
        SystemTime::UNIX_EPOCH
            .checked_add(Duration::from_secs(unix_timestamp.into()))
            .ok_or(
                Error::Noise(
                    format!(
                        "Cannot convert unix timestamp ({}) to system time",
                        unix_timestamp
                    )
                    .to_string(),
                )
                .into(),
            )
    }
}

/// Helper struct for performing the actual signature of the relevant parts of the certificate
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct SignedPart {
    header: SignedPartHeader,
    pubkey: StaticPublicKey,
    authority_public_key: ed25519_dalek::PublicKey,
}

impl SignedPart {
    pub fn new(
        header: SignedPartHeader,
        pubkey: StaticPublicKey,
        authority_public_key: ed25519_dalek::PublicKey,
    ) -> Self {
        Self {
            header,
            pubkey,
            authority_public_key,
        }
    }

    fn serialize_to_buf(&self) -> Result<BytesMut> {
        let mut signed_part_writer = BytesMut::new().writer();
        v2::serialization::to_writer(&mut signed_part_writer, self)?;
        Ok(signed_part_writer.into_inner())
    }

    /// Generates the actual ed25519_dalek::Signature that is ready to be embedded into the certificate
    pub fn sign_with(&self, keypair: &ed25519_dalek::Keypair) -> Result<ed25519_dalek::Signature> {
        assert_eq!(
            keypair.public,
            self.authority_public_key,
            "BUG: Signing Authority public key ({}) inside the certificate doesn't match the key \
             we are trying to sign with (its public key is: {})",
            EncodedEd25519PublicKey::new(keypair.public),
            EncodedEd25519PublicKey::new(self.authority_public_key)
        );

        let signed_part_buf = self.serialize_to_buf()?;
        Ok(keypair.sign(&signed_part_buf[..]))
    }

    /// Verifies the specifed `signature` against this signed part
    fn verify(&self, signature: &ed25519_dalek::Signature) -> Result<()> {
        let signed_part_buf = self.serialize_to_buf()?;
        self.authority_public_key
            .verify_strict(&signed_part_buf[..], signature)?;
        Ok(())
    }

    fn verify_expiration(&self, now: SystemTime) -> Result<()> {
        self.header.verify_expiration(now)
    }
}

/// The payload message that will be appended to the handshake message to proof static key
/// authenticity
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct SignatureNoiseMessage {
    header: SignedPartHeader,
    signature: ed25519_dalek::Signature,
}

impl SignatureNoiseMessage {
    pub fn serialize_to_writer<T: std::io::Write>(&self, writer: &mut T) -> Result<()> {
        v2::serialization::to_writer(writer, self)?;
        Ok(())
    }

    pub fn serialize_to_bytes_mut(&self) -> Result<BytesMut> {
        let mut writer = BytesMut::new().writer();
        self.serialize_to_writer(&mut writer)?;

        let serialized_signature_noise_message = writer.into_inner();

        Ok(serialized_signature_noise_message)
    }
}

/// Deserialization implementation
impl TryFrom<&[u8]> for SignatureNoiseMessage {
    type Error = Error;

    fn try_from(data: &[u8]) -> Result<Self> {
        v2::serialization::from_slice(data)
            .map_err(|e| Error::from(e))
            .map_err(Into::into)
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::{
        super::{generate_keypair, StaticKeypair},
        *,
    };
    use rand::rngs::OsRng;
    const TEST_CERT_VALIDITY: Duration = Duration::from_secs(3600);

    // Helper that builds a `SignedPart` (as a base e.g. for a noise message or a certificate),
    // testing authority `ed25519_dalek::Keypair` (that actually generated the signature) and the
    // `ed25519_dalek::Signature`
    pub(crate) fn build_test_signed_part_and_auth() -> (
        SignedPart,
        ed25519_dalek::Keypair,
        StaticKeypair,
        ed25519_dalek::Signature,
    ) {
        let mut csprng = OsRng {};
        let to_be_signed_keypair =
            generate_keypair().expect("BUG: cannot generate noise static keypair");
        let authority_keypair = ed25519_dalek::Keypair::generate(&mut csprng);
        let header = SignedPartHeader::with_duration(TEST_CERT_VALIDITY)
            .expect("BUG: cannot prepare certificate header");

        let signed_part = SignedPart::new(
            header,
            to_be_signed_keypair.public.clone(),
            authority_keypair.public,
        );
        let signature = signed_part
            .sign_with(&authority_keypair)
            .expect("BUG: cannot sign");
        (
            signed_part,
            authority_keypair,
            to_be_signed_keypair,
            signature,
        )
    }

    #[test]
    fn header_time_validity_is_valid() {
        let header = SignedPartHeader::with_duration(TEST_CERT_VALIDITY)
            .expect("BUG: cannot build certificate header");
        header
            .verify_expiration(SystemTime::now() + Duration::from_secs(10))
            .expect("BUG: certificate should be evaluated as valid!");
    }

    #[test]
    fn header_time_validity_not_yet_valid() {
        let header = SignedPartHeader::with_duration(TEST_CERT_VALIDITY)
            .expect("BUG: cannot build certificate header");
        let result = header.verify_expiration(SystemTime::now() - Duration::from_secs(10));
        assert!(
            result.is_err(),
            "BUG: Certificate not evaluated as not valid yet: {:?}",
            result
        );
    }

    #[test]
    fn header_time_validity_is_expired() {
        let header = SignedPartHeader::with_duration(TEST_CERT_VALIDITY)
            .expect("BUG: cannot build certificate header");
        let result = header
            .verify_expiration(SystemTime::now() + TEST_CERT_VALIDITY + Duration::from_secs(10));
        assert!(
            result.is_err(),
            "BUG: Certificate not evaluated as expired: {:?}",
            result
        );
    }

    #[test]
    fn signature_noise_message_serialization() {
        let (signed_part, authority_keypair, _static_keypair, _signature) =
            build_test_signed_part_and_auth();

        let noise_message = SignatureNoiseMessage {
            header: signed_part.header.clone(),
            signature: signed_part
                .sign_with(&authority_keypair)
                .expect("BUG: cannot sign"),
        };

        let mut serialized_noise_message_writer = BytesMut::new().writer();
        noise_message
            .serialize_to_writer(&mut serialized_noise_message_writer)
            .expect("BUG: cannot serialize signature noise message");

        let serialized_noise_message_buf = serialized_noise_message_writer.into_inner();
        let deserialized_noise_message =
            SignatureNoiseMessage::try_from(&serialized_noise_message_buf[..])
                .expect("BUG: cannot deserialize signature noise message");

        assert_eq!(
            noise_message, deserialized_noise_message,
            "Signature noise messages don't match each other after serialization cycle"
        )
    }
}
