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

use std::iter::repeat;

use futures::stream::StreamExt;

use ii_async_compat::tokio;

use super::*;
use ii_stratum::test_utils;
use ii_stratum::v1;
use ii_stratum::v2;

/// Simulates incoming message by converting it into a `Frame` and running the deserialization
/// chain from that point on
async fn v2_simulate_incoming_message<M>(translation: &mut V2ToV1Translation, message: M)
where
    M: TryInto<v2::Frame, Error = ii_stratum::error::Error>,
{
    // create a tx frame, we won't send it but only extract the pure data (as it implements the deref trait)
    let frame: v2::Frame = message.try_into().expect("Could not serialize message");

    let msg = v2::build_message_from_frame(frame).expect("Deserialization failed");
    msg.accept(translation).await;
}

async fn v1_simulate_incoming_message<M>(translation: &mut V2ToV1Translation, message: M)
where
    M: TryInto<v1::Frame, Error = ii_stratum::error::Error>,
{
    // create a tx frame, we won't send it but only extract the pure data (as it implements the deref trait) as if it arrived to translation
    let frame: v1::Frame = message.try_into().expect("Deserialization failed");

    let msg = v1::build_message_from_frame(frame).expect("Deserialization failed");
    msg.accept(translation).await;
}

async fn v2_verify_generated_response_message(v2_rx: &mut mpsc::Receiver<v2::Frame>) {
    // Pickup the response and verify it
    let v2_response_tx_frame = v2_rx.next().await.expect("At least 1 message was expected");

    // This is specific for the unit test only: Instead of sending the message via some
    // connection, the test case will deserialize it and inspect it using the identity
    // handler from test utils
    let v2_response =
        v2::build_message_from_frame(v2_response_tx_frame).expect("Deserialization failed");
    // verify the response using testing identity handler
    v2_response
        .accept(&mut test_utils::v2::TestIdentityHandler)
        .await;
}

async fn v1_verify_generated_response_message(v1_rx: &mut mpsc::Receiver<v1::Frame>) {
    // Pickup the response and verify it
    // TODO add timeout
    let frame = v1_rx.next().await.expect("At least 1 message was expected");

    let msg = v1::build_message_from_frame(frame).expect("Deserialization failed");
    msg.accept(&mut test_utils::v1::TestIdentityHandler).await;
}

#[tokio::test]
async fn test_client_reconnect_translate() {
    let (v1_tx, _v1_rx) = mpsc::channel(1);
    let (v2_tx, mut v2_rx) = mpsc::channel(1);
    let mut tr_options = V2ToV1TranslationOptions::default();
    tr_options.propagate_reconnect_downstream = true;
    let mut translation = V2ToV1Translation::new(v1_tx, v2_tx, tr_options);

    v1_simulate_incoming_message(
        &mut translation,
        test_utils::v1::build_client_reconnect_request_message(),
    )
    .await;

    v2_verify_generated_response_message(&mut v2_rx).await;
}

/// This test simulates incoming connection to the translation and verifies that the translation
/// emits corresponding V1 or V2 messages
/// TODO we need a way to detect that translation is not responding and the entire test should fail
#[tokio::test]
async fn test_setup_connection_translate() {
    let (v1_tx, mut v1_rx) = mpsc::channel(1);
    let (v2_tx, mut v2_rx) = mpsc::channel(1);
    let mut translation = V2ToV1Translation::new(v1_tx, v2_tx, Default::default());

    v2_simulate_incoming_message(&mut translation, test_utils::v2::build_setup_connection()).await;
    // Setup mining connection should result into: mining.configure
    v1_verify_generated_response_message(&mut v1_rx).await;
    v1_simulate_incoming_message(
        &mut translation,
        test_utils::v1::build_configure_ok_response_message(),
    )
    .await;
    v2_verify_generated_response_message(&mut v2_rx).await;

    // Opening a channel should result into: V1 generating a subscribe request
    v2_simulate_incoming_message(&mut translation, test_utils::v2::build_open_channel()).await;
    // Opening a channel should result into: V1 generating a subscribe and authorize requests
    v1_verify_generated_response_message(&mut v1_rx).await;
    v1_verify_generated_response_message(&mut v1_rx).await;

    // Subscribe response
    v1_simulate_incoming_message(
        &mut translation,
        test_utils::v1::build_subscribe_ok_response_message(),
    )
    .await;
    // Authorize response
    v1_simulate_incoming_message(
        &mut translation,
        test_utils::v1::build_authorize_ok_response_message(),
    )
    .await;

    // SetDifficulty notification before completion
    v1_simulate_incoming_message(
        &mut translation,
        test_utils::v1::build_set_difficulty_request_message(),
    )
    .await;
    // Now we should have a successfully open channel
    v2_verify_generated_response_message(&mut v2_rx).await;

    v1_simulate_incoming_message(
        &mut translation,
        test_utils::v1::build_mining_notify_request_message(),
    )
    .await;
    // Expect NewMiningJob
    v2_verify_generated_response_message(&mut v2_rx).await;
    // Expect SetNewPrevHash
    v2_verify_generated_response_message(&mut v2_rx).await;
    // Ensure that the V1 job has been registered
    let submit_template = V1SubmitTemplate {
        job_id: v1::messages::JobId::from_str(&test_utils::v1::MINING_NOTIFY_JOB_ID),
        time: test_utils::common::MINING_WORK_NTIME,
        version: test_utils::common::MINING_WORK_VERSION,
    };

    let registered_submit_template = translation
        .v2_to_v1_job_map
        .get(&0)
        .expect("No mining job with V2 ID 0");
    assert_eq!(
        submit_template,
        registered_submit_template.clone(),
        "New Mining Job ID not registered!"
    );

    // Send SubmitShares
    v2_simulate_incoming_message(&mut translation, test_utils::v2::build_submit_shares()).await;
    // Expect mining.submit to be generated
    v1_verify_generated_response_message(&mut v1_rx).await;
    // Simulate mining.submit response (true)
    v1_simulate_incoming_message(
        &mut translation,
        test_utils::v1::build_mining_submit_ok_response_message(),
    )
    .await;
    // Expect SubmitSharesSuccess to be generated
    v2_verify_generated_response_message(&mut v2_rx).await;
    // });
}

#[test]
fn test_diff_1_bitcoin_target() {
    // Difficulty 1 target in big-endian format
    let difficulty_1_target_bytes: [u8; 32] = [
        0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];
    let expected_difficulty_1_target_uint256 =
        uint::U256::from_big_endian(&difficulty_1_target_bytes);

    assert_eq!(
        expected_difficulty_1_target_uint256,
        V2ToV1Translation::DIFF1_TARGET,
        "Bitcoin difficulty 1 targets don't match exp: {:x?}, actual:{:x?}",
        expected_difficulty_1_target_uint256,
        V2ToV1Translation::DIFF1_TARGET
    );
}

#[test]
fn test_parse_client_reconnect() {
    use serde_json::Value;
    use v1::messages::ClientReconnect;

    assert_eq!(
        (Str0_255::from_str(""), 0),
        V2ToV1Translation::parse_client_reconnect(&ClientReconnect(vec![]))
            .expect(r#"Could not parse reconnect message without arguments"#)
    );

    // lower boundary case
    assert_eq!(
        (Str0_255::from_str(""), 0),
        V2ToV1Translation::parse_client_reconnect(&ClientReconnect(vec![
            Value::String("".into()),
            Value::String("0".into()),
            Value::String("1".into()),
        ]))
        .expect(r#"Could not parse boundary_case with host="" and port="0"#)
    );

    // lower boundary case
    assert_eq!(
        (Str0_255::from_str(""), 0),
        V2ToV1Translation::parse_client_reconnect(&ClientReconnect(vec![
            Value::String("".into()),
            Value::Number(0.into()),
            Value::Number(1.into()),
        ]))
        .expect(r#"Could not parse boundary_case with host="" and integeral port=0"#)
    );

    // random case
    assert_eq!(
        (Str0_255::from_str("some_host"), 1000),
        V2ToV1Translation::parse_client_reconnect(&ClientReconnect(vec![
            Value::String("some_host".into()),
            Value::Number(1000.into()),
        ]))
        .expect(r#"Could not parse regular case with host="some_host" and integeral port=1000"#)
    );

    // upper boundary case
    assert_eq!(
        (
            Str0_255::from_string(repeat("h").take(255).collect::<String>()),
            65535
        ),
        V2ToV1Translation::parse_client_reconnect(&ClientReconnect(vec![
            Value::String(repeat("h").take(255).collect::<String>()),
            Value::String("65535".into()),
            Value::String("1".into()),
        ]))
        .expect(
            r#"Could not parse boundary_case with longest valid host and string port="65535"."#
        )
    );

    // upper boundary cases
    assert_eq!(
        (
            Str0_255::from_string(repeat("h").take(255).collect::<String>()),
            65535
        ),
        V2ToV1Translation::parse_client_reconnect(&ClientReconnect(vec![
            Value::String(repeat("h").take(255).collect::<String>()),
            Value::Number(65535.into()),
            Value::Number(1.into()),
        ]))
        .expect(
            r#"Could not parse boundary_case with longest valid host and integeral port=65535."#
        )
    );

    // non-ascii host name
    assert_eq!(
        (Str0_255::from_str("😊"), 1000),
        V2ToV1Translation::parse_client_reconnect(&ClientReconnect(vec![
            Value::String("😊".into()),
            Value::Number(1000.into()),
        ]))
        .expect("Could not parse non-ascii utf-8 host-name string")
    );
}

/// Test port number overflow, hostname overflow, invalid port number string, hexadecimal string
#[test]
fn test_client_reconnect_parsing_with_invalid_arguments() {
    use v1::messages::ClientReconnect;

    if let Ok((_host, _port)) = V2ToV1Translation::parse_client_reconnect(&ClientReconnect(vec![
        Value::String("some_host".into()),
        Value::String("65536".into()), // invalid range
    ])) {
    } else if let Ok((_host, _port)) =
        V2ToV1Translation::parse_client_reconnect(&ClientReconnect(vec![
            Value::String("some_host".into()),
            Value::Number(65536.into()), // invalid range
        ]))
    {
        panic!("invalid port number integer not detected: {:?}", _port);
    } else if let Ok((_host, _port)) =
        V2ToV1Translation::parse_client_reconnect(&ClientReconnect(vec![
            Value::String(repeat("h").take(256).collect::<String>()), // too long host name
            Value::Number(1000.into()),
        ]))
    {
        panic!("too long hostname not detected: {:?}", _host);
    } else if let Ok((_host, _port)) =
        V2ToV1Translation::parse_client_reconnect(&ClientReconnect(vec![
            Value::String("some_host".into()),
            Value::String("bad_non-numeric-port_description".into()), // invalid port string
        ]))
    {
        panic!("invalid non-numeric port value not detected: {:?}", _port);
    } else if let Ok((_host, _port)) =
        V2ToV1Translation::parse_client_reconnect(&ClientReconnect(vec![
            Value::String("some_host".into()),
            Value::Array(vec![1000.into()]), // invalid data type
        ]))
    {
        panic!("invalid port data type not detected")
    } else if let Ok((_host, _port)) =
        V2ToV1Translation::parse_client_reconnect(&ClientReconnect(vec![
            Value::Number(10.into()), // invalid data type
            Value::Number(1000.into()),
        ]))
    {
        panic!("invalid host name data type not detected")
    }
}
