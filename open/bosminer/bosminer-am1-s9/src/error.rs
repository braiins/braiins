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

//! The Antminer S9 errors

use failure::{Backtrace, Context, Fail};
use std::fmt::{self, Debug, Display};

use ii_async_i2c as i2c;
use ii_sensors as sensor;
use std::io;
use sysfs_gpio;

pub struct Error {
    inner: Context<ErrorKind>,
}

#[derive(Clone, Eq, PartialEq, Debug, Fail)]
pub enum ErrorKind {
    /// General error used for more specific input/output error.
    #[fail(display = "{}", _0)]
    General(String),

    /// Standard input/output error.
    #[fail(display = "IO: {}", _0)]
    Io(String),

    /// Unexpected version of something.
    #[fail(display = "Unexpected {} version: {}, expected: {}", _0, _1, _2)]
    UnexpectedVersion(String, String, String),

    /// Error concerning hashboard with specific index.
    #[fail(display = "Hashboard {}: {}", _0, _1)]
    Hashboard(usize, String),

    /// Error concerning hashchip enumeration.
    #[fail(display = "Enumeration: {}", _0)]
    ChipEnumeration(String),

    /// Baud rate errors.
    #[fail(display = "Baud rate: {}", _0)]
    BaudRate(String),

    /// GPIO errors.
    #[fail(display = "GPIO: {}", _0)]
    Gpio(String),

    /// I2C errors.
    #[fail(display = "I2C: {}", _0)]
    I2c(String),

    /// I2C errors.
    #[fail(display = "Linux I2C: {}", _0)]
    LinuxI2c(String),

    /// Power controller errors.
    #[fail(display = "Power: {}", _0)]
    Power(String),

    /// Error from hashchain manager.
    #[fail(display = "HashChain Manager: {}", _0)]
    HashChainManager(HashChainManager),

    /// Error when halting.
    #[fail(display = "Halt: {}", _0)]
    Halt(String),

    /// Error when dealing with sensors.
    #[fail(display = "Sensors: {}", _0)]
    Sensors(String),

    /// Error related to bosminer-antminer crate
    #[fail(display = "Antminer error: {}", _0)]
    Antminer(String),
}

#[derive(Clone, Eq, PartialEq, Debug, Fail)]
pub enum HashChainManager {
    #[fail(display = "HashChain parameters not set")]
    ParamsNotSet,
}

/// Implement Fail trait instead of use Derive to get more control over custom type.
/// The main advantage is customization of Context type which allows conversion of
/// any error types to this custom error with general error kind by calling context
/// method on any result type.
impl Fail for Error {
    fn cause(&self) -> Option<&dyn Fail> {
        self.inner.cause()
    }

    fn backtrace(&self) -> Option<&Backtrace> {
        self.inner.backtrace()
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.inner, f)
    }
}

impl Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Debug::fmt(&self.inner, f)
    }
}

impl Error {
    pub fn kind(&self) -> ErrorKind {
        self.inner.get_context().clone()
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Self {
            inner: Context::new(kind),
        }
    }
}

impl From<Context<ErrorKind>> for Error {
    fn from(inner: Context<ErrorKind>) -> Self {
        Self { inner }
    }
}

impl From<Context<String>> for Error {
    fn from(context: Context<String>) -> Self {
        Self {
            inner: context.map(|info| ErrorKind::General(info)),
        }
    }
}

impl From<String> for Error {
    fn from(msg: String) -> Self {
        ErrorKind::General(msg).into()
    }
}

impl From<std::num::ParseIntError> for Error {
    fn from(e: std::num::ParseIntError) -> Self {
        let msg = e.to_string();
        Self {
            inner: e.context(ErrorKind::General(msg)),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        let msg = e.to_string();
        Self {
            inner: e.context(ErrorKind::Io(msg)),
        }
    }
}

impl From<sysfs_gpio::Error> for Error {
    fn from(gpio_error: sysfs_gpio::Error) -> Self {
        let msg = gpio_error.to_string();
        Self {
            inner: gpio_error.context(ErrorKind::Gpio(msg)),
        }
    }
}

impl From<i2c::Error> for Error {
    fn from(e: i2c::Error) -> Self {
        ErrorKind::I2c(format!("{:?}", e)).into()
    }
}

impl From<sensor::Error> for Error {
    fn from(e: sensor::Error) -> Self {
        ErrorKind::Sensors(format!("{:?}", e)).into()
    }
}

impl From<ii_linux_async_i2c::Error> for Error {
    fn from(e: ii_linux_async_i2c::Error) -> Self {
        ErrorKind::LinuxI2c(format!("{:?}", e)).into()
    }
}

impl From<bosminer_antminer::error::Error> for Error {
    fn from(e: bosminer_antminer::error::Error) -> Self {
        ErrorKind::Antminer(format!("{:?}", e)).into()
    }
}

/// A specialized `Result` type bound to [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
