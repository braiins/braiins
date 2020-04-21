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

//! Module that represents stratum protocol errors

use ii_async_compat::{tokio, tokio_util};
use std::{self, fmt, io};
use thiserror::Error;

#[derive(Error, Debug)] //TODO: We lost Clone PartialEq and Eq, is this important?
pub enum Error {
    /// Input/Output error.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Handshake error: {0}")]
    Handshake(String),

    /// Line Codec error.
    #[error("Lines Codec error: {0}")]
    LinesCodec(#[from] tokio_util::codec::LinesCodecError),

    /// Errors emitted by serde
    #[error("Serde JSON: {0}")]
    Serde(#[from] serde_json::error::Error),

    /// General error used for more specific .
    #[error("General error: {0}")]
    General(String),

    /// Unexpected version of something.
    #[error("Unexpected {0} version: {1}, expected: {2}")]
    UnexpectedVersion(String, String, String),

    #[error("Noise handshake error: {0}")]
    Noise(String),

    #[error("Noise protocol error: {0}")]
    NoiseProtocol(#[from] snow::error::Error),

    #[error("Noise signature error: {0}")]
    NoiseSignature(#[from] ed25519_dalek::SignatureError),

    #[error("Noise base58 error: {0}")]
    NoiseEncoding(#[from] bs58::decode::Error),

    /// Stratum version 1 error
    #[error("V1 error: {0}")]
    V1(#[from] super::v1::error::Error),

    /// Stratum version 2 error
    #[error("V2 error: {0}")]
    V2(#[from] super::v2::error::Error),

    /// Stratum version 2 serialization error
    #[error("V2 serialization error: {0}")]
    V2Serialization(#[from] super::v2::serialization::Error),

    /// Hex Decode error
    #[error("Hex value decoding error: {0}")]
    HexDecode(#[from] hex::FromHexError),

    /// Invalid Bitcoin hash
    #[error("Invalid Bitcoin hash: {0}")]
    BitcoinHash(#[from] bitcoin_hashes::Error),

    /// Timeout error
    #[error("Timeout error: {0}")]
    Timeout(#[from] tokio::time::Elapsed),

    /// Format error
    #[error("Format error: {0}")]
    Format(#[from] fmt::Error),

    /// Utf8 error
    #[error("Error decoding UTF-8 string: {0}")]
    Utf8(#[from] std::str::Utf8Error),
}

impl From<&str> for Error {
    fn from(info: &str) -> Self {
        Error::General(info.to_string())
    }
}

impl From<String> for Error {
    fn from(info: String) -> Self {
        Error::General(info)
    }
}

/// A specialized `Result` type bound to [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod test {
    use super::super::v2::error::Error as V2Error;
    use super::*;

    #[test]
    fn test_error_display_with_inner_error() {
        let inner_msg = "Usak is unknown";
        let inner = V2Error::UnknownMessage(inner_msg.into());
        let err = Error::V2(inner);
        let msg = err.to_string();
        assert!(msg.contains(inner_msg));
    }
}
