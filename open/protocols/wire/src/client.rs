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

use std::fmt;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs as StdToSocketAddrs};
use std::str::FromStr;
use std::time::{Duration, Instant};
use std::vec;

use tokio::net::TcpStream;
use tokio::time;

use ii_async_compat::prelude::*;
use thiserror::Error;

#[derive(Error, PartialEq, Eq, Debug)]
#[error("Invalid endpoint address syntax (host:port)")]
pub struct AddressParseError;

/// This is a tuple of a `String` holding a hostname/IP address
/// and a port number. `Address` can be parsed from a string in the
/// `"hostanem:port"` format using `from_str()` (from the `FromStr` trait).
///
/// `Address` does not and can not imeplement Tokio's asynchronous `ToSockAddrs` because `ToSockAddrs` is sealed in Tokio,
/// instead, use the `as_ref()` method to get `(&str, u16)` which implements `tokio::net::ToSockAddrs`.
/// `Address` does implement the synchronous `std::net::ToSockAddrs` though, which is useful for
/// server sockets.
///
/// You can also use `connect()` to create a `Connection` directly.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Address(pub String, pub u16);

impl Address {
    /// Returns a `(&str, u16)` tuple, where the `&str` references the `String`.
    /// This is used to pass `(&str, u16)` to connection methods which require `ToSockAddrs`.
    pub fn as_ref(&self) -> (&str, u16) {
        (self.0.as_str(), self.1)
    }

    /// Create a `TcpStream` connected to this address
    pub async fn connect(&self) -> io::Result<TcpStream> {
        TcpStream::connect(self.as_ref()).await
    }
}

impl StdToSocketAddrs for Address {
    type Iter = vec::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
        self.as_ref().to_socket_addrs()
    }
}

impl From<(String, u16)> for Address {
    fn from(tuple: (String, u16)) -> Self {
        Address(tuple.0, tuple.1)
    }
}

impl<'a> From<(&'a str, u16)> for Address {
    fn from(tuple: (&'a str, u16)) -> Self {
        Address(tuple.0.to_string(), tuple.1)
    }
}

impl From<Address> for (String, u16) {
    fn from(addr: Address) -> Self {
        (addr.0, addr.1)
    }
}

impl<'a> From<&'a Address> for (&'a str, u16) {
    fn from(addr: &'a Address) -> Self {
        addr.as_ref()
    }
}

impl FromStr for Address {
    type Err = AddressParseError;

    fn from_str(src: &str) -> Result<Self, AddressParseError> {
        let col_pos = src.find(':').ok_or(AddressParseError)?;
        if col_pos == 0 {
            return Err(AddressParseError);
        }

        let port = u16::from_str(&src[col_pos + 1..]).map_err(|_| AddressParseError)?;
        let host = src[..col_pos].to_string();

        Ok(Address(host, port))
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.0, self.1)
    }
}

/// Backoff generation for `ReConnection`.
pub trait Backoff: Send + fmt::Debug {
    /// Called by `ReConnection` when next sleep duration is required.
    fn next(&mut self) -> Duration;

    /// Called by `ReConnection` when a connection is (re-)established
    /// so that the backoff type can eg. reset its state.
    fn reset(&mut self);
}

/// Default `Backoff` implementation, based on the fibonacci sequence.
#[derive(Debug)]
struct DefaultBackoff {
    current: u32,
    prev: u32,
    unit: Duration,
    max: Duration,
}

impl DefaultBackoff {
    /// Constructor. As `DefaultBackoff` produces numbers of the fibonacci sequence,
    /// each is multiplied by `unit` before being returned. In the `Default` implementation,
    /// the `unit` is 100 ms. This generates backoff of 100, 100, 200, 300, 500, ... milliseconds.
    ///
    /// `max` is the maximum backoff ever returned (after multiplication by `unit`). In the `Default`
    /// implementation this is 5 seconds.
    pub fn new(unit: Duration, max: Duration) -> Self {
        let mut res = DefaultBackoff {
            current: 0,
            prev: 0,
            unit,
            max,
        };

        res.reset();
        res
    }
}

impl Backoff for DefaultBackoff {
    fn next(&mut self) -> Duration {
        let current = self.current;
        let res = self.unit * current;

        if res >= self.max {
            self.max
        } else {
            let prev = self.prev;
            self.current = current + prev;
            self.prev = current;
            res
        }
    }

    fn reset(&mut self) {
        self.current = 1;
        self.prev = 0;
    }
}

impl Default for DefaultBackoff {
    fn default() -> Self {
        Self::new(Duration::from_millis(100), Duration::from_secs(5))
    }
}

/// The error type returned when a connection attempt fails.
///
/// The structure holds a few items related to backoff state
/// and the original connection I/O error.
#[derive(Error, Debug)]
pub struct AttemptError {
    /// Duration since the this (failed) till next time the `next()`
    /// will at the soonest perform another connection attempt.
    pub next_attempt_in: Duration,
    /// Number of failed reconnection attempts, including this one,
    /// since the connection broke.
    pub retries: u32,
    /// The instant when the first re-connection attempt was started after the connection broke.
    /// (You can use this to compute how long it has been in total since the connection broke
    /// by subtracting this from `Instant::now()`)
    pub start_time: Instant,
    /// The I/O error returned by the underlying `Connection`.
    #[source]
    pub error: io::Error,
}

impl AttemptError {
    fn new(next_attempt_in: Duration, retries: u32, start_time: Instant, error: io::Error) -> Self {
        Self {
            next_attempt_in,
            retries,
            start_time,
            error,
        }
    }
}

impl fmt::Display for AttemptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let next_in = self.next_attempt_in.as_millis() as u64;
        write!(
            f,
            "Connection attempt error, attempt #{}, next in: {}ms",
            self.retries, next_in
        )
    }
}

#[derive(Debug)]
pub struct Client {
    /// Server address to connect to
    addr: Address,
    /// Backoff strategy trait object
    backoff: Box<dyn Backoff>,
    /// When connection attempt fails, current time (Instant) and a backoff Duration
    /// are saved here, this is used by next() to compute delay time before attempting
    /// connection when called next time.
    next_delay: Option<(Instant, Duration)>,
    /// Number of connection retries, reset when connection is established
    retries: u32,
    /// Time of the first attempt, reset if the connection is established,
    /// see AttemptError::start_time
    start_time: Option<Instant>,
}

impl Client {
    /// Create a new `Client` that will connect to `addr` with
    /// the default backoff.
    pub fn new(addr: Address) -> Self {
        Self::with_backoff(addr, DefaultBackoff::default())
    }

    /// Create a new `Client` that will connecto to `addr` with
    /// the supplied backoff.
    pub fn with_backoff<B>(addr: Address, backoff: B) -> Self
    where
        B: Backoff + 'static,
    {
        Self {
            addr,
            backoff: Box::new(backoff),
            next_delay: None,
            retries: 0,
            start_time: None,
        }
    }

    pub fn set_addr(&mut self, addr: Address) {
        self.addr = addr;
    }

    pub fn set_backoff<B: Backoff + 'static>(&mut self, backoff: B) {
        self.backoff = Box::new(backoff);
    }

    pub async fn next(&mut self) -> Result<TcpStream, AttemptError> {
        self.start_time.get_or_insert(Instant::now());

        if let Some((when, delay)) = self.next_delay.take() {
            let since_last_attempt = Instant::now().duration_since(when);
            if delay > since_last_attempt {
                time::delay_for(delay - since_last_attempt).await;
            }
        }

        match self.addr.connect().await {
            Ok(conn) => {
                self.backoff.reset();
                self.retries = 0;
                self.start_time = None;
                Ok(conn)
            }
            Err(err) => {
                let backoff = self.backoff.next();
                self.next_delay = Some((Instant::now(), backoff));
                self.retries += 1;
                let start_time = self.start_time.unwrap();
                Err(AttemptError::new(backoff, self.retries, start_time, err))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_address_parsing() {
        assert_eq!(
            Address::from_str("localhost:443"),
            Ok(Address("localhost".into(), 443))
        );
        assert_eq!(
            Address::from_str("127.0.0.1:443"),
            Ok(Address("127.0.0.1".into(), 443))
        );

        assert_eq!(Address::from_str("localhost:xxx"), Err(AddressParseError));
        assert_eq!(Address::from_str("localhost"), Err(AddressParseError));
        assert_eq!(Address::from_str("localhost:"), Err(AddressParseError));
        assert_eq!(Address::from_str(":"), Err(AddressParseError));
        assert_eq!(Address::from_str(":123"), Err(AddressParseError));
    }
}
