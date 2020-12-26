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

use async_trait::async_trait;
use bytes::BytesMut;
use serde::Serialize;
use std::convert::{TryFrom, TryInto};
use std::fmt::Debug;
use std::str::FromStr;

use ii_async_compat::bytes;
use ii_logging::macros::*;

use super::common::*;
use crate::v1::{framing::*, messages::*, rpc::*, ExtraNonce1, Handler, HexBytes, MessageId};

pub const MINING_CONFIGURE_REQ_JSON: &str = concat!(
    r#"{"id":0,"method":"mining.configure","#,
    r#""params":[["version-rolling"],"#,
    r#"{"version-rolling.mask":"1fffe000","version-rolling.min-bit-count":16}]}"#
);

pub fn build_configure_request() -> Rpc {
    build_request_message(Some(0), build_configure())
}

pub fn build_configure() -> Configure {
    let v = VersionRolling::new(
        crate::BIP320_N_VERSION_MASK,
        crate::BIP320_N_VERSION_MAX_BITS,
    );

    let mut configure = Configure::new();
    configure
        .add_feature(v)
        .expect("Could not add Configure feature");

    configure
}

pub const MINING_CONFIGURE_OK_RESP_JSON: &str = concat!(
    r#"{"id":0,"error":null,"result": {"version-rolling":true,"#,
    r#""version-rolling.mask":"1fffe000"}}"#
);

pub fn build_configure_ok_response_message() -> Rpc {
    let cfg: ConfigureResult =
        serde_json::from_str(r#"{"version-rolling":true,"version-rolling.mask":"1fffe000"}"#)
            .expect("configure_ok_response deserialization failed");
    trace!("build_configure_ok_response_message() {:?}", cfg);
    build_result_response_message(0, cfg)
}

/// Testing subscribe request in a dense form without any spaces
pub const MINING_SUBSCRIBE_REQ_JSON: &str = concat!(
    r#"{"id":1,"method":"mining.subscribe","#,
    r#""params":["Braiins OS 2019-06-05",null,"stratum.slushpool.com:3333",null]}"#
);

const EXTRA_NONCE_1: &str = "6c6f010000000c";
const EXTRA_NONCE_2_SIZE: usize = 4;

fn build_request_message<T>(id: MessageId, payload: T) -> Rpc
where
    T: TryInto<RequestPayload> + std::fmt::Debug,
    <T as std::convert::TryInto<RequestPayload>>::Error: std::fmt::Debug,
{
    Rpc::from(Request {
        id,
        payload: payload.try_into().expect("Cannot serialize request"),
    })
}

pub fn build_subscribe_request_frame() -> Rpc {
    build_request_message(Some(1), build_subscribe())
}

pub fn build_subscribe() -> Subscribe {
    let hostname_port: String = format!("{}:{}", String::from(POOL_URL), POOL_PORT);
    Subscribe(
        Some(MINER_SW_SIGNATURE.into()), // agent_signature
        None,                            // extra_nonce1
        Some(hostname_port),             // url
        None,                            // port
    )
}

/// Random broken request
pub const MINING_BROKEN_REQ_JSON: &str = concat!(
    r#"{"id":1,"method":"mining.none_existing","#,
    r#""params":["10","12"]}"#
);

/// Subscribe success response in a dense form without any spaces
/// TODO: find out how to fill in extra nonce 1 and extra nonce 2 size from predefined constants
pub const MINING_SUBSCRIBE_OK_RESULT_JSON: &str = concat!(
    r#"{"id":1,"#,
    r#""result":[[],"6c6f010000000c",4],"#,
    r#""error":null}"#
);

fn build_result_response_message<T: Serialize>(id: u32, result: T) -> Rpc {
    Rpc::from(Response {
        id,
        payload: ResponsePayload {
            result: Some(
                StratumResult::new_from(result).expect("Cannot build test response message"),
            ),
            error: None,
        },
    })
}

/// Special case for simple 'OK' response
fn build_ok_response_message(id: u32) -> Rpc {
    build_result_response_message(id, BooleanResult(true))
}

pub fn build_subscribe_ok_response_message() -> Rpc {
    build_result_response_message(1, build_subscribe_ok_result())
}

pub fn build_authorize_ok_response_message() -> Rpc {
    build_ok_response_message(2)
}

pub fn build_mining_submit_ok_response_message() -> Rpc {
    build_ok_response_message(3)
}

pub fn build_subscribe_ok_result() -> SubscribeResult {
    SubscribeResult(
        vec![],
        ExtraNonce1(HexBytes::try_from(EXTRA_NONCE_1).expect("Cannot parse extra nonce 1")),
        EXTRA_NONCE_2_SIZE,
    )
}

/// Subscribe success response in a dense form without any spaces
/// TODO: find out how to fill in extra nonce 1 and extra nonce 2 size from predefined constants
pub const MINING_SUBSCRIBE_MALFORMED_RESULT_JSON: &str =
    r#"{"id":1,"result":["01650f001f25ea",4],"error":null}"#;

/// Testing error response in a dense form without any spaces
pub const STRATUM_ERROR_JSON: &str = r#"{"id":1,"result":null,"error":[20,"Other/Unknown",null]}"#;

pub fn build_stratum_error() -> StratumError {
    StratumError(20, "Other/Unknown".into(), None)
}

pub fn build_stratum_err_response() -> Rpc {
    Rpc::from(Response {
        id: 1,
        payload: ResponsePayload {
            result: None,
            error: Some(build_stratum_error()),
        },
    })
}

pub const MINING_SET_DIFFICULTY_JSON: &str =
    r#"{"id":null,"method":"mining.set_difficulty","params":[4.0]}"#;

pub fn build_set_difficulty_request_message() -> Rpc {
    build_request_message(None, build_set_difficulty())
}

pub fn build_set_difficulty() -> SetDifficulty {
    SetDifficulty([4f32])
}

pub const MINING_NOTIFY_JOB_ID: &str = "ahoj";
pub const MINING_NOTIFY_JSON: &str = concat!(
    r#"{"#,
    r#""id":null,"method":"mining.notify","#,
    r#""params":["ahoj","13f46cc7bf03a16697170dbb9d15680b7e75fcf10846037f171d7f6b00000000","01000000010000000000000000000000000000000000000000000000000000000000000000ffffffff44026d0cfabe6d6dc22da09055dabfce93b90fec9c53cbec5ace52248db605efe1d2f2c1bfc8f1260100000000000000","e91d012f736c7573682f000000000200f2052a010000001976a914505b9f58045298b98a7af6333445098ac700ac3088ac0000000000000000266a24aa21a9ede2f61c3f71d1defd3fa999dfa36953755c690689799962b48bebd836974e8cf900000000",[],"20000000","1d00ffff","5d10bc0a",false]"#,
    r#"}"#,
);

/// This is merkle root for the job specified via MINING_NOTIFY_JSON
/// TODO: verify that this merkle root is correct assuming extra nonce 1 = 0, extra nonce 2 size = 4,
/// and extra nonce 2 =0
pub const MINING_NOTIFY_MERKLE_ROOT: &str =
    "91176a137779ca8a591fa94461210fc2d62e607c7aef93ed38dff510f0d946a2";
//"085958174f0dceccf57a5e5c49641fbf821a0d2029b144fca97affeb7b561834";
// Original merkle root (doesn't match)
//    "a4eedd0736c8e5d316bbd77f683ce932e96f4cc8ac54159bdc8575903f0013f3";

pub fn build_mining_notify_request_message() -> Rpc {
    build_request_message(None, build_mining_notify())
}

pub fn build_mining_notify() -> Notify {
    let deserialized = Rpc::from_str(MINING_NOTIFY_JSON).expect("Cannot parse mining job");

    let notify = if let Rpc::Request(req) = deserialized {
        Notify::try_from(req).expect("Cannot build mining notify message")
    } else {
        panic!("Wrong notification message");
    };

    notify
}

pub const MINING_SUBMIT_JSON: &str = concat!(
    r#"{"id":3,"method":"mining.submit","#,
    // TODO the correct share extra nonce 2 is 01000000, we have to replace the sample job
    // completely with a new none that has extra nonce 2 == 0
    r#""params":["braiins.worker0","ahoj","00000000","5d10bc0a","0443c37b","00000000"]"#,
    r#"}"#
);

pub fn build_mining_submit_request_message() -> Rpc {
    build_request_message(None, build_mining_submit())
}

pub fn build_mining_submit() -> Submit {
    let deserialized = Rpc::from_str(MINING_SUBMIT_JSON).expect("Cannot parse mining job");

    let submit = if let Rpc::Request(req) = deserialized {
        Submit::try_from(req).expect("Cannot build mining submit message")
    } else {
        panic!("Wrong notification message");
    };

    submit
}

pub const MINING_AUTHORIZE_JSON: &str =
    r#"{"id":2,"method":"mining.authorize","params":["braiins.worker0",""]}"#;

pub fn build_authorize_request_message() -> Rpc {
    build_request_message(Some(1), build_authorize())
}

pub fn build_authorize() -> Authorize {
    Authorize(USER_CREDENTIALS.to_string(), "".to_string())
}

pub const MINING_AUTHORIZE_OK: &str = r#"{"id": 1,"error":null,"result":true}"#;

pub const CLIENT_RECONNECT_JSON: &str =
    r#"{"id":1,"method":"client.reconnect","params":["stratum.slushpool.com", 3333, 1]}"#;

pub fn build_client_reconnect_request_message() -> Rpc {
    build_request_message(Some(1), build_client_reconnect())
}

pub fn build_client_reconnect() -> ClientReconnect {
    let deserialized =
        Rpc::from_str(CLIENT_RECONNECT_JSON).expect("Cannot parse reconnect message");
    let reconnect = if let Rpc::Request(req) = deserialized {
        ClientReconnect::try_from(req).expect("Cannot build reconnect message")
    } else {
        panic!("Wrong reconnect message");
    };
    reconnect
}

/// Message payload visitor that compares the payload of the visited message (e.g. after
/// deserialization test) with the payload built.
/// This handler should be used in tests to verify that serialization and deserialization yield the
/// same results
pub struct TestIdentityHandler;
//pub struct TestIdentityHandler(fn()->Strat);

impl TestIdentityHandler {
    /// Performs 2 checks:
    /// - if the provided message payload matches the one expected by the test (provided by
    /// `build_payload` function
    /// - whether the `full_message` after serialization matches the expected `json_message` JSON
    /// representation
    fn visit_and_check<P, F>(
        &mut self,
        id: &MessageId,
        payload: &P,
        build_payload: F,
        full_message: Rpc,
        json_message: &str,
    ) where
        P: Debug + PartialEq,
        F: FnOnce() -> P,
    {
        // Build expected payload for verifying correct deserialization
        let expected_payload = build_payload();
        trace!("V1 TestIdentityHandler: Message ID {:?} {:?}", id, payload);
        assert_eq!(expected_payload, *payload, "Message payloads don't match");

        // Build frame from the provided Rpc message and use its serialization for test evaluation
        let message_frame: Frame = full_message
            .try_into()
            .expect("BUG: Cannot build frame from Rpc");
        let mut serialized_frame = BytesMut::new();
        message_frame
            .serialize(&mut serialized_frame)
            .expect("BUG: Cannot serialize frame");
        assert_eq!(
            json_message,
            std::str::from_utf8(&serialized_frame[..])
                .expect("BUG: Can't convert serialized message to str"),
            "Serialized messages don't match"
        );
    }

    fn visit_and_check_request<P, F>(
        &mut self,
        id: &MessageId,
        payload: &P,
        build_payload: F,
        json_message: &str,
    ) where
        P: Debug + PartialEq + Clone + TryInto<RequestPayload>,
        <P as std::convert::TryInto<RequestPayload>>::Error: std::fmt::Debug,
        F: FnOnce() -> P,
    {
        self.visit_and_check(
            id,
            payload,
            build_payload,
            build_request_message(*id, payload.clone()),
            json_message,
        );
    }
}

#[async_trait]
impl Handler for TestIdentityHandler {
    async fn visit_stratum_result(&mut self, id: &MessageId, payload: &StratumResult) {
        self.visit_and_check(
            id,
            payload,
            || {
                StratumResult::new_from(build_subscribe_ok_result())
                    .expect("Cannot convert to stratum result")
            },
            build_result_response_message(id.expect("Message ID missing"), payload),
            MINING_SUBSCRIBE_OK_RESULT_JSON,
        );
    }

    async fn visit_configure(&mut self, id: &MessageId, payload: &Configure) {
        self.visit_and_check_request(id, payload, build_configure, MINING_CONFIGURE_REQ_JSON);
    }

    async fn visit_subscribe(&mut self, id: &MessageId, payload: &Subscribe) {
        self.visit_and_check_request(id, payload, build_subscribe, MINING_SUBSCRIBE_REQ_JSON);
    }

    async fn visit_authorize(&mut self, id: &MessageId, payload: &Authorize) {
        self.visit_and_check_request(id, payload, build_authorize, MINING_AUTHORIZE_JSON);
    }

    async fn visit_set_difficulty(&mut self, id: &MessageId, payload: &SetDifficulty) {
        self.visit_and_check_request(
            id,
            payload,
            build_set_difficulty,
            MINING_SET_DIFFICULTY_JSON,
        );
    }

    async fn visit_notify(&mut self, id: &MessageId, payload: &Notify) {
        self.visit_and_check_request(id, payload, build_mining_notify, MINING_NOTIFY_JSON);
    }

    async fn visit_submit(&mut self, id: &MessageId, payload: &Submit) {
        self.visit_and_check_request(id, payload, build_mining_submit, MINING_SUBMIT_JSON);
    }
}

/// A complete list of all requests in this module for massive testing
/// TODO Is it possible to use inventory crate to collect these?
pub const V1_TEST_REQUESTS: &[&str] = &[
    MINING_SUBSCRIBE_REQ_JSON,
    MINING_NOTIFY_JSON,
    MINING_AUTHORIZE_JSON,
    MINING_SUBSCRIBE_REQ_JSON,
    MINING_SET_DIFFICULTY_JSON,
    MINING_SUBMIT_JSON,
];
