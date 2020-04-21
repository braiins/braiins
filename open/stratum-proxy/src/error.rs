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

//! Module that represents custom stratum proxy errors

use std;
use std::io;
use thiserror::Error;

use ii_async_compat::prelude::*;

#[derive(Error, Debug)]
pub enum Error {
    /// General error used for more specific errors.
    #[error("General error: {0}")]
    General(String),

    /// Stratum protocol error.
    #[error("Stratum error: {0}")]
    Stratum(#[from] ii_stratum::error::Error),

    /// Bitcoin Hashes error.
    #[error("Bitcoin Hashes error: {0}")]
    BitcoinHashes(#[from] bitcoin_hashes::error::Error),

    /// Input/Output error.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Timeout error.
    #[error("Timeout error: {0}")]
    Timeout(#[from] tokio::time::Elapsed),

    /// Utf8 error
    #[error("Error decoding UTF-8 string: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    /// Json Error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Connection Attempt Error
    #[error("Client connection attempt error: {0}")]
    ClientAttempt(#[from] ii_wire::AttemptError),

    /// File content error
    #[error("Invalid content of key/certificate file: {0}")]
    InvalidFile(String),
}

impl<T> From<futures::channel::mpsc::TrySendError<T>> for Error
where
    T: Send + Sync + 'static,
{
    fn from(e: futures::channel::mpsc::TrySendError<T>) -> Self {
        Error::Io(io::Error::new(io::ErrorKind::Other, e))
    }
}

impl From<&str> for Error {
    fn from(msg: &str) -> Self {
        Error::General(msg.to_string())
    }
}

impl From<String> for Error {
    fn from(msg: String) -> Self {
        Error::General(msg)
    }
}

/// A specialized `Result` type bound to [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
