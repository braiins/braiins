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

use crate::error;

use ii_stratum::v2;

use url::Url;

use std::convert::TryFrom;
use std::fmt;

use failure::ResultExt;

pub const URL_JAVA_SCRIPT_REGEX: &'static str =
    "(?:drain|(?:stratum2?\\+tcp(?:\\+insecure)?)):\\/\\/[\\w\\.-]+(?::\\d+)?(?:\\/[\\dA-HJ-NP-Za-km-z]+)?";

#[derive(Clone, Debug)]
pub enum Protocol {
    Drain,
    StratumV1,
    StratumV2(v2::noise::auth::EncodedEd25519PublicKey),
    StratumV2Insecure,
}

impl Protocol {
    pub const SCHEME_DRAIN: &'static str = "drain";
    pub const SCHEME_STRATUM_V1: &'static str = "stratum+tcp";
    pub const SCHEME_STRATUM_V2: &'static str = "stratum2+tcp";
    pub const SCHEME_STRATUM_V2_INSECURE: &'static str = "stratum2+tcp+insecure";

    pub const DEFAULT_PORT_DRAIN: u16 = 0;
    pub const DEFAULT_PORT_STRATUM_V1: u16 = 3333;
    pub const DEFAULT_PORT_STRATUM_V2: u16 = 3336;
    pub const DEFAULT_PORT_STRATUM_V2_INSECURE: u16 = 3336;

    pub fn default_port(&self) -> u16 {
        match self {
            Self::Drain => Self::DEFAULT_PORT_DRAIN,
            Self::StratumV1 => Self::DEFAULT_PORT_STRATUM_V1,
            Self::StratumV2(_) => Self::DEFAULT_PORT_STRATUM_V2,
            Self::StratumV2Insecure => Self::DEFAULT_PORT_STRATUM_V2_INSECURE,
        }
    }

    /// Helper that builds authority public key
    fn get_upstream_auth_public_key_from_string(
        public_key: &str,
    ) -> error::Result<v2::noise::auth::EncodedEd25519PublicKey> {
        v2::noise::auth::EncodedEd25519PublicKey::try_from(public_key.to_string())
            .context(format!(
                "invalid upstream authority public key: {}",
                public_key
            ))
            .map_err(Into::into)
    }

    pub fn parse(scheme: &str, path: &str) -> error::Result<Self> {
        Ok(match scheme {
            Self::SCHEME_DRAIN => Self::Drain,
            Self::SCHEME_STRATUM_V1 => Self::StratumV1,
            Self::SCHEME_STRATUM_V2 => {
                let upstream_authority_public_key = match path.get(1..) {
                    Some(s) => Self::get_upstream_auth_public_key_from_string(s)?,
                    None => Err(error::ErrorKind::Client(format!(
                        "missing upstream authority key for securing {} connection",
                        scheme
                    )))?,
                };
                Self::StratumV2(upstream_authority_public_key)
            }
            Self::SCHEME_STRATUM_V2_INSECURE => Self::StratumV2Insecure,
            _ => Err(error::ErrorKind::Client(format!(
                "unknown protocol '{}'",
                scheme
            )))?,
        })
    }

    pub fn scheme(&self) -> &'static str {
        match self {
            Self::Drain => Self::SCHEME_DRAIN,
            Self::StratumV1 => Self::SCHEME_STRATUM_V1,
            Self::StratumV2(_) => Self::SCHEME_STRATUM_V2,
            Self::StratumV2Insecure => Self::SCHEME_STRATUM_V2_INSECURE,
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Protocol::Drain => write!(f, "Drain"),
            Protocol::StratumV1 => write!(f, "Stratum V1"),
            Protocol::StratumV2(public_key) => {
                write!(f, "Stratum V2 (authority key: {})", public_key)
            }
            Protocol::StratumV2Insecure => write!(f, "Stratum V2 Insecure"),
        }
    }
}

pub struct UserInfo<'a> {
    pub user: &'a str,
    pub password: Option<&'a str>,
}

impl<'a> UserInfo<'a> {
    pub const DELIMITER: char = ':';

    pub fn new(user: &'a str, password: Option<&'a str>) -> Self {
        Self { user, password }
    }

    /// Parse user and password from user info (user[:password])
    pub fn parse(value: &'a str) -> Self {
        let user_info: Vec<_> = value.rsplitn(2, Self::DELIMITER).collect();
        let mut user_info = user_info.iter().rev();

        let user = user_info.next().expect("BUG: missing user");
        let password = user_info.next().map(|value| *value);

        Self { user, password }
    }
}

/// Contains basic information about client used for obtaining jobs for solving.
#[derive(Clone, Debug)]
pub struct Descriptor {
    pub protocol: Protocol,
    pub enabled: bool,
    pub user: String,
    pub password: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    // Currently used only for `#xnsub`: `stratum+tcp://equihash.eu.nicehash.com:3357#xnsub`
    pub fragment: Option<String>,
}

impl Descriptor {
    pub fn port(&self) -> u16 {
        match self.port {
            Some(value) => value,
            None => self.protocol.default_port(),
        }
    }

    pub fn get_url(&self, protocol: bool, port: bool, user: bool) -> String {
        let mut result = if protocol {
            self.protocol.scheme().to_string() + "://"
        } else {
            String::new()
        };
        if user {
            result += format!("{}@", self.user).as_str();
        }
        result += self.host.as_str();
        match self.port {
            Some(value) if port => result += format!(":{}", value).as_str(),
            _ => {}
        }

        result
    }

    #[inline]
    pub fn get_full_url(&self) -> String {
        self.get_url(true, true, true)
    }

    /// Create client `Descriptor` from information provided by user.
    pub fn create(url: &str, user_info: &UserInfo, enabled: bool) -> error::Result<Self> {
        let url = Url::parse(url).context(error::ErrorKind::Client("invalid URL".to_string()))?;

        let protocol = Protocol::parse(url.scheme(), url.path())?;
        let host = url
            .host()
            .ok_or(error::ErrorKind::Client("missing hostname".to_string()))?
            .to_string();
        let port = url.port();

        // Parse fragment part
        let fragment = url.fragment().map(|s| s.to_string());

        Ok(Descriptor {
            protocol,
            enabled,
            user: user_info.user.to_string(),
            password: user_info.password.map(|value| value.to_string()),
            host,
            port,
            fragment,
        })
    }

    /// Detect extranonce subscribe in the URL fragment stored within this descriptor. This is
    /// for Stratum V1 protocol support.
    pub fn detect_xnsub(&self) -> bool {
        self.host.find(".nicehash.com").is_some()
            || self
                .fragment
                .as_ref()
                .and_then(|fragment| fragment.find("xnsub"))
                .is_some()
    }
}
