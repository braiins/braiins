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

//! Stratum V1/V2 Client/Server integration test that:
//! - listens on a dedicated port (9001 for V1, 9002 for V2)
//! - a sample client connects that sends and expects a response from it
//!
//! NOTE: currently this test must be run with --nocapture flag as there is no reasonable way of
//! communicating any failures/panics to the test harness.

use std::convert::TryInto;
use std::net::SocketAddr;
use std::str::FromStr;

use ii_async_compat::prelude::*;

use ii_stratum::error::Error;
use ii_stratum::test_utils;
use ii_stratum::v1;
use ii_stratum::v2;
use ii_stratum_proxy::server;
use ii_wire::{Address, Connection, Server};

mod utils;

static ADDR: &'static str = "127.0.0.1";
static PORT_V1: u16 = 9001;
static PORT_V2: u16 = 9002;
static PORT_V2_FULL: u16 = 9003;

#[tokio::test]
async fn test_v2server() {
    // FIXME: unwraps

    let addr = Address(ADDR.into(), PORT_V2);
    let mut server = Server::bind(&addr).expect("BUG: cannot bind to address");

    // Spawn server task that reacts to any incoming message and responds
    // with SetupConnectionSuccess
    tokio::spawn(async move {
        let mut conn = Connection::<v2::Framing>::new(server.next().await.unwrap().unwrap());
        let frame = conn.next().await.unwrap().unwrap();
        let msg = v2::build_message_from_frame(frame).expect("Failed to build message from frame");
        // test handler verifies that the message
        msg.accept(&mut test_utils::v2::TestIdentityHandler).await;

        // test response frame
        conn.send(
            test_utils::v2::build_setup_connection_success()
                .try_into()
                .expect("BUG: Cannot convert to frame"),
        )
        .await
        .expect("BUG: Could not send message");
    });

    // Testing client
    let mut connection: Connection<v2::Framing> = addr
        .connect()
        .await
        .unwrap_or_else(|e| panic!("Could not connect to {}: {}", addr, e))
        .into();
    connection
        .send(
            test_utils::v2::build_setup_connection()
                .try_into()
                .expect("BUG: Cannot convert to frame"),
        )
        .await
        .expect("BUG: Could not send message");

    let response_frame = connection.next().await.unwrap().unwrap();
    let response = v2::build_message_from_frame(response_frame)
        .expect("Failed to build response message from frame");
    response
        .accept(&mut test_utils::v2::TestIdentityHandler)
        .await;
}

// WIP attempt to generalize
//fn test_server<F, P>(client_handler: &P::Handler, server_handler: &P::Handler, port: usize)
//where
//    F: ii_wire::Framing,
//    P: ii_wire::ProtocolBase,
//    <F as ii_wire::Framing>::Error: std::fmt::Debug,
//    <F as ii_wire::Framing>::Tx: std::convert::From<ii_wire::Frame>,
//    <F as ii_wire::Framing>::Rx:
//{
//    ii_async_compat::run(
//        async {
//            let addr = format!("{}:{}", ADDR, port).parse().unwrap();
//
//            let mut server = Server::<F>::bind(&addr).unwrap();
//
//            // Spawn server task that reacts to any incoming message and responds
//            // with SetupMiningConnectionSuccess
//            tokio::spawn_async(async move {
//                let mut conn = await!(server.next()).unwrap().unwrap();
//                let msg:ii_wire::Message<P> = await!(conn.next()).unwrap().unwrap();
//                // test handler verifies that the message
//                msg.accept(server_handler);
//
//                // test response frame
//                let response: Frame =
//                    RpcResponse(test_utils::v1::build_subscribe_ok_rpc_response())
//                        .try_into()
//                        .expect("Cannot serialize response");
//
//                await!(conn.send(response));
//            });
//
//            // Testing client
//            let mut connection =
//                await!(Connection::<F>::connect(addr)).expect("Could not connect");
//            let request: TxFrame = RpcRequest(test_utils::v1::build_subscribe_rpc_request())
//                .try_into()
//                .expect("Cannot serialize request frame");
//            await!(connection.send(request));
//
//            let response = await!(connection.next()).unwrap().unwrap();
//            response.accept(client_handler);
//        }
//    );
//}

fn v1server_task(addr: SocketAddr) -> impl Future<Output = ()> {
    let mut server = Server::bind(&addr).expect("BUG: cannot bind to address");

    async move {
        while let Some(conn) = server.next().await {
            let mut conn = Connection::<v1::Framing>::new(
                conn.expect("BUG server did not provide connection"),
            );

            while let Some(frame) = conn.next().await {
                let frame = frame.expect("Receiving frame failed");
                let msg: ii_stratum::Message<v1::Protocol> =
                    v1::build_message_from_frame(frame).expect("Cannot deserialize frame");
                // test handler verifies that the message
                msg.accept(&mut test_utils::v1::TestIdentityHandler).await;

                // test response frame
                let response = test_utils::v1::build_subscribe_ok_response_message();
                conn.send(response.try_into().expect("BUG: Cannot convert to frame"))
                    .await
                    .expect("BUG: Could not send response");
            }
        }
    }
}

/// TODO this test is currently work in progress and is disfunctional. Code needs to be consolidated
/// And factor out common code with V2 server as attempted above.
#[tokio::test]
#[ignore]
async fn test_v1server() {
    let addr = format!("{}:{}", ADDR, PORT_V1).parse().unwrap();

    // Spawn server task that reacts to any incoming message and responds
    // with SetupConnectionSuccess
    tokio::spawn(v1server_task(addr));

    // Testing client
    let mut connection = Connection::<v1::Framing>::connect(&addr)
        .await
        .unwrap_or_else(|e| panic!("Could not connect to {}: {}", addr, e));

    let request = test_utils::v1::build_subscribe_request_frame();
    connection
        .send(request.try_into().expect("BUG: Cannot convert to frame"))
        .await
        .expect("BUG: Could not send request");

    let response_frame = connection.next().await.unwrap().unwrap();
    let response = v1::build_message_from_frame(response_frame).expect("Cannot deserialize frame");
    response
        .accept(&mut test_utils::v1::TestIdentityHandler)
        .await;
}

async fn test_v2_client(server_addr: &Address) {
    // Test client for V2
    utils::backoff(50, 4, move || {
        async move {
            let mut conn: Connection<v2::Framing> = server_addr.connect().await?.into();

            // Initialize server connection
            conn.send(
                test_utils::v2::build_setup_connection()
                    .try_into()
                    .expect("BUG: Cannot convert to frame"),
            )
            .await
            .expect("BUG: Could not send message");

            match conn.next().await.expect("BUG: proxy do not respond") {
                Ok(frame) => {
                    v2::build_message_from_frame(frame)
                        .expect("BUG: received invalid message")
                        .header;
                }
                Err(_) => panic!("BUG: Could not receive message"),
            }

            Result::<(), Error>::Ok(())
        }
    })
    .await
    .unwrap_or_else(|e| panic!("Could not connect to {}: {}", server_addr, e));
}

#[tokio::test]
async fn test_v2server_full() {
    let addr_v1 = Address::from_str("stratum.slushpool.com:3333")
        .expect("BUG: cannot build stratum v1 address");
    let addr_v2 = Address(ADDR.into(), PORT_V2_FULL);

    let v2server =
        server::ProxyServer::listen(addr_v2.clone(), addr_v1, server::handle_connection, None)
            .expect("BUG: Could not bind v2server");
    let mut v2server_quit = v2server.quit_channel();

    tokio::spawn(v2server.run());
    test_v2_client(&addr_v2).await;

    // Signal the server to shut down
    let _ = v2server_quit.try_send(());
    // TODO kill v1 test server
}
