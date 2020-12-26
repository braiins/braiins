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

//! Definition of all Stratum V1 messages

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::convert::{TryFrom, TryInto};

use super::error::ErrorKind;

use crate::error::{Result, ResultExt};
use crate::v1::{
    rpc::{self, Method},
    ExtraNonce1, HexBytes, HexU32Be, PrevHash, Protocol,
};

#[cfg(test)]
pub mod test;

/// Generates implementation of conversions for various protocol request messages
macro_rules! impl_conversion_request {
    ($request:tt, $method:path, $handler_fn:tt) => {
        // NOTE: $request and $handler_fn need to be tt because of https://github.com/dtolnay/async-trait/issues/46

        impl TryFrom<$request> for rpc::RequestPayload {
            type Error = crate::error::Error;

            fn try_from(msg: $request) -> Result<Self> {
                let params = serde_json::to_value(msg).context("Cannot parse request")?;

                Ok(Self {
                    method: $method,
                    params,
                })
            }
        }

        impl TryFrom<rpc::Request> for $request {
            type Error = crate::error::Error;

            fn try_from(req: rpc::Request) -> std::result::Result<Self, Self::Error> {
                // Invariant: it's caller's responsibility to ensure not to pass wrong request
                // for conversion
                assert_eq!(req.payload.method, $method);

                serde_json::from_value(req.payload.params).map_err(Into::into)
            }
        }

        #[async_trait]
        impl crate::payload::SerializablePayload<Protocol> for $request {
            async fn accept(
                &self,
                id: &<Protocol as crate::Protocol>::Header,
                handler: &mut <Protocol as crate::Protocol>::Handler,
            ) {
                handler.$handler_fn(id, self).await;
            }

            fn serialize_to_writer(&self, _writer: &mut dyn std::io::Write) -> Result<()> {
                panic!(
                    "BUG: serialization of partial request without Rpc not supported {:?}",
                    self
                );
            }
        }
    };
}

macro_rules! impl_conversion_response {
    ($response:ty) => {
        impl TryFrom<$response> for rpc::ResponsePayload {
            type Error = crate::error::Error;

            fn try_from(resp: $response) -> Result<rpc::ResponsePayload> {
                let result = rpc::StratumResult(
                    serde_json::to_value(resp).context("Cannot parse response")?,
                );

                Ok(rpc::ResponsePayload {
                    result: Some(result),
                    error: None,
                })
            }
        }

        impl TryFrom<rpc::Response> for $response {
            type Error = crate::error::Error;

            fn try_from(resp: rpc::Response) -> Result<Self> {
                let result = resp
                    .payload
                    .result
                    .ok_or(ErrorKind::Json("No result".into()))?;
                <$response>::try_from(&result)
            }
        }

        impl TryFrom<&rpc::StratumResult> for $response {
            type Error = crate::error::Error;

            fn try_from(result: &rpc::StratumResult) -> Result<Self> {
                // TODO this is needs to be fixed within the deserialization stack with regards
                // to the visitor pattern. We shouldn't clone any part of the incoming message
                // However, since the result is being passed by reference
                serde_json::from_value(result.0.clone())
                    .context(format!("Failed to parse response ({:?})", result))
                    .map_err(Into::into)
            }
        }
    };
}

/// Version rolling mask has a new type to provide one consistent place
/// that determines the exact serialization format of it
/// Mask bits are allocated as per BIP320
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct VersionMask(pub HexU32Be);

/// Version rolling configuration extension that follows the model in BIP310
/// Miner requests a certain mask and minimum amount of bits
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct VersionRolling {
    /// Mask bits are allocated as per BIP320
    #[serde(rename = "version-rolling.mask")]
    pub mask: VersionMask,
    /// Minimum required number of bits for rolling
    #[serde(rename = "version-rolling.min-bit-count")]
    pub min_bit_count: usize,
}

impl VersionRolling {
    pub fn new(mask: u32, min_bit_count: usize) -> Self {
        Self {
            mask: VersionMask(HexU32Be(mask)),
            min_bit_count,
        }
    }
}

impl TryInto<(String, serde_json::Value)> for VersionRolling {
    type Error = crate::error::Error;

    fn try_into(self) -> Result<(String, serde_json::Value)> {
        Ok((
            "version-rolling".to_string(),
            serde_json::to_value(self).context("JSON error")?,
        ))
    }
}

/// Mining configure
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct Configure(pub Vec<String>, pub serde_json::Value);

impl Configure {
    /// Constructs an empty configuration
    pub fn new() -> Self {
        Self {
            0: vec![],
            1: serde_json::value::Value::Object(serde_json::map::Map::new()),
        }
    }

    /// Simplifies adding new feature to the current map
    pub fn add_feature<T>(&mut self, feature: T) -> Result<()>
    where
        T: TryInto<(String, serde_json::Value), Error = crate::error::Error>,
    {
        let (feature_name, feature_map) = feature.try_into()?;
        self.0.push(feature_name);

        // Merge the feature into the current configuration map
        if let serde_json::Value::Object(ref mut configure_map) = self.1 {
            if let serde_json::Value::Object(ref feature_map) = feature_map {
                for (k, v) in feature_map {
                    configure_map.insert(k.clone(), v.clone());
                }
            }
        }

        Ok(())
    }
}
impl_conversion_request!(Configure, Method::Configure, visit_configure);

#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct ConfigureResult(pub serde_json::Value);

impl_conversion_response!(ConfigureResult);

/// Extranonce subscriptionMessage
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct ExtranonceSubscribe();

impl_conversion_request!(
    ExtranonceSubscribe,
    Method::ExtranonceSubscribe,
    visit_extranonce_subscribe
);

/// SetExtranonce message (sent if we subscribed with `ExtranonceSubscribe`)
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct SetExtranonce(pub ExtraNonce1, pub usize);

impl SetExtranonce {
    pub fn extra_nonce_1(&self) -> &ExtraNonce1 {
        &self.0
    }

    pub fn extra_nonce_2_size(&self) -> usize {
        self.1
    }
}

impl_conversion_request!(SetExtranonce, Method::SetExtranonce, visit_set_extranonce);

/// Compounds all data required for mining subscription
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct Subscribe(
    pub Option<String>,
    pub Option<ExtraNonce1>,
    pub Option<String>,
    pub Option<String>,
);

impl Subscribe {
    //    pub fn new(agent_signature: Option<String>, extra_nonce1: ExtraNonce1, url: String, port: String) -> Self {
    //        Self(agent, extra_nonce1, url, port)
    //    }
    pub fn agent_signature(&self) -> Option<&String> {
        self.0.as_ref()
    }
    pub fn extra_nonce1(&self) -> Option<&ExtraNonce1> {
        self.1.as_ref()
    }
    pub fn url(&self) -> Option<&String> {
        self.2.as_ref()
    }
    pub fn port(&self) -> Option<&String> {
        self.3.as_ref()
    }
}

// Subscribe::try_from()
//  FIXME: verify signature, url, and port?
//  let agent_signature =
//      req.param_to_string(0, ErrorKind::Subscribe("Invalid signature".into()))?;
//  let url = req.param_to_string(2, ErrorKind::Subscribe("Invalid pool URL".into()))?;
//  let port = req.param_to_string(3, ErrorKind::Subscribe("Invalid TCP port".into()))?;
impl_conversion_request!(Subscribe, Method::Subscribe, visit_subscribe);

/// Subscription response
/// TODO: Do we need to track any subscription ID's or anyhow validate those fields?
/// see StratumError for reasons why this structure doesn't have named fields
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct SubscribeResult(pub Vec<serde_json::Value>, pub ExtraNonce1, pub usize);

impl SubscribeResult {
    pub fn subscriptions(&self) -> &Vec<serde_json::Value> {
        &self.0
    }

    pub fn extra_nonce_1(&self) -> &ExtraNonce1 {
        &self.1
    }

    pub fn extra_nonce_2_size(&self) -> usize {
        self.2
    }
}

// TODO write a test case for parsing incorrect response
impl_conversion_response!(SubscribeResult);

/// A boolean result
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct BooleanResult(pub bool);

impl_conversion_response!(BooleanResult);

/// Subscription response
/// TODO: Do we need to track any subscription ID's or anyhow validate those fields?
/// see StratumError for reasons why this structure doesn't have named fields
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct Authorize(pub String, pub String);

impl Authorize {
    pub fn name(&self) -> &String {
        &self.0
    }

    pub fn password(&self) -> &String {
        &self.1
    }
}

impl_conversion_request!(Authorize, Method::Authorize, visit_authorize);

/// Difficulty value set by the upstream stratum server
/// Note, that we explicitly enforce 1 one element array so that serde doesn't flatten the
/// 'params' JSON array to a single value, eliminating the array completely.
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct SetDifficulty(pub [f32; 1]);

impl SetDifficulty {
    pub fn value(&self) -> f32 {
        self.0[0]
    }
}

impl_conversion_request!(SetDifficulty, Method::SetDifficulty, visit_set_difficulty);
//#[derive(Deserialize)]
//struct Helper(#[serde(with = "DurationDef")] Duration);
//
//let dur = serde_json::from_str(j).map(|Helper(dur)| dur)?;

#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct JobId(String);

impl JobId {
    pub fn from_str(job_id: &str) -> Self {
        Self(String::from(job_id))
    }
}

/// Leading part of the coinbase transaction
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct CoinBase1(HexBytes);

/// Trailing part of the coinbase transaction
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct CoinBase2(HexBytes);

/// Merkle branch of transaction hashes leading to coinbase
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct MerkleBranch(Vec<HexBytes>);

/// Version field of Bitcoin block header
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct Version(HexU32Be);

/// Network difficulty target
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct Bits(HexU32Be);

/// Network time
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct Time(HexU32Be);

/// New mining job notification
/// TODO generate the field accessors
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct Notify(
    JobId,
    PrevHash,
    CoinBase1,
    CoinBase2,
    MerkleBranch,
    Version,
    Bits,
    Time,
    bool,
);

// TODO consider making the attributes return new type references, it would be less prone to typos
impl Notify {
    pub fn job_id(&self) -> &str {
        &(self.0).0
    }

    pub fn prev_hash(&self) -> &[u8] {
        self.1.as_ref()
    }

    pub fn coin_base_1(&self) -> &[u8] {
        &((self.2).0).0
    }

    pub fn coin_base_2(&self) -> &[u8] {
        &((self.3).0).0
    }

    pub fn merkle_branch(&self) -> &Vec<HexBytes> {
        &(self.4).0
    }

    pub fn version(&self) -> u32 {
        ((self.5).0).0
    }

    pub fn bits(&self) -> u32 {
        ((self.6).0).0
    }

    pub fn time(&self) -> u32 {
        ((self.7).0).0
    }

    pub fn clean_jobs(&self) -> bool {
        self.8
    }
}

impl_conversion_request!(Notify, Method::Notify, visit_notify);

/// Server may arbitrarily adjust version mask
/// Note, that we explicitly enforce 1 one element array so that serde doesn't flatten the
/// 'params' JSON array to a single value, eliminating the array completely.
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct SetVersionMask(pub [VersionMask; 1]);

impl SetVersionMask {
    pub fn value(&self) -> u32 {
        ((self.0[0]).0).0
    }
}

impl_conversion_request!(
    SetVersionMask,
    Method::SetVersionMask,
    visit_set_version_mask
);

/// Combined username and worker
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct UserName(String);

/// Extra nonce 2, note the underlying serialization type
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct ExtraNonce2(HexBytes);

/// Nonce for the block
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct Nonce(HexU32Be);

/// New mining job notification
/// TODO generate the field accessors
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct Submit(UserName, JobId, ExtraNonce2, Time, Nonce, Version);

impl Submit {
    pub fn new(
        user_name: String,
        job_id: JobId,
        extra_nonce2: &[u8],
        time: u32,
        nonce: u32,
        version: u32,
    ) -> Self {
        Self(
            UserName(user_name),
            job_id,
            ExtraNonce2(HexBytes(extra_nonce2.into())),
            Time(HexU32Be(time)),
            Nonce(HexU32Be(nonce)),
            Version(HexU32Be(version)),
        )
    }

    pub fn user_name(&self) -> &String {
        &(self.0).0
    }

    pub fn job_id(&self) -> &String {
        &(self.1).0
    }

    pub fn extra_nonce_2(&self) -> &[u8] {
        &((self.2).0).0
    }

    pub fn time(&self) -> u32 {
        ((self.3).0).0
    }

    pub fn nonce(&self) -> u32 {
        ((self.4).0).0
    }

    pub fn version(&self) -> u32 {
        ((self.5).0).0
    }
}

impl_conversion_request!(Submit, Method::Submit, visit_submit);

/// Server initiated message requiring client to perform a reconnect, all fields are optional and
/// we don't know which of them the server sends
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct ClientReconnect(pub Vec<serde_json::Value>);

impl ClientReconnect {
    pub fn host(&self) -> Option<&serde_json::Value> {
        if self.0.len() > 0 {
            Some(&self.0[0])
        } else {
            None
        }
    }

    pub fn port(&self) -> Option<&serde_json::Value> {
        if self.0.len() > 1 {
            Some(&self.0[1])
        } else {
            None
        }
    }

    pub fn wait_time(&self) -> Option<&serde_json::Value> {
        if self.0.len() > 2 {
            Some(&self.0[2])
        } else {
            None
        }
    }
}

impl_conversion_request!(
    ClientReconnect,
    Method::ClientReconnect,
    visit_client_reconnect
);
