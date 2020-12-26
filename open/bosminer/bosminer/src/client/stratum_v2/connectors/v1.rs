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

//! Adapter module for connecting to stratum V1 endpoints by providing translation

use futures::channel::mpsc;
use tokio::net::TcpStream;

use ii_async_compat::prelude::*;
use ii_async_compat::select;
use ii_logging::macros::*;
use ii_stratum::{v1, v2};
use ii_stratum_proxy::translation::{V2ToV1Translation, V2ToV1TranslationOptions};

use pin_project::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::error;

/// Wrapper that extablishes translated V2->V1 connection
/// Note: explicitely derive Copy, so that the instance can be consumed and moved into the future.
/// All is really being copied is the public key
#[derive(Copy, Clone, Debug)]
pub(crate) struct Connector {
    /// Options required by the `TranslationHandler`
    translation_options: V2ToV1TranslationOptions,
}

impl Connector {
    pub fn new(extranonce_subscribe: bool) -> Self {
        Self {
            translation_options: V2ToV1TranslationOptions {
                try_enable_xnsub: extranonce_subscribe,
                // We want to receive the reconnect messages from the translation component so
                // that the connection can be dropped and reconnected somewhere else
                propagate_reconnect_downstream: true,
            },
        }
    }

    pub async fn connect(
        self,
        connection: TcpStream,
    ) -> error::Result<(v2::DynFramedSink, v2::DynFramedStream)> {
        trace!("Stratum V2 noise connector: {:?}, {:?}", connection, self);

        let v1_framed_connection = ii_wire::Connection::<v1::Framing>::new(connection).into_inner();

        let (translation_handler, v2_translation_receiver, v2_translation_sender) =
            TranslationHandler::new(v1_framed_connection, self.translation_options);
        tokio::spawn(async move {
            let status = translation_handler.run().await;
            debug!("V2->V1 translation terminated: {:?}", status);
        });

        Ok((
            Pin::new(Box::new(V2ConnectorSender::new(v2_translation_sender))),
            V2ConnectorReceiver::new(v2_translation_receiver).boxed(),
        ))
    }

    /// Converts the connector into a closure that provides the connect future for later
    /// evaluation once an actual connection has been established
    pub fn into_connector_fn(self) -> super::DynConnectFn {
        Box::new(move |connection| self.connect(connection).boxed())
    }
}

/// Helper wrapper that adapts the errors from the Sender to errors compatible with
/// v2::DynFramedSink
/// TODO: review if we can get rid of this boiler plate code and find out a way how to map the
/// errors also consider placing the code into stratum crate
#[pin_project]
#[derive(Debug)]
struct V2ConnectorSender {
    #[pin]
    inner: mpsc::Sender<v2::Frame>,
}

impl V2ConnectorSender {
    fn new(inner: mpsc::Sender<v2::Frame>) -> Self {
        Self { inner }
    }
}

impl Sink<<v2::Framing as ii_wire::Framing>::Tx> for V2ConnectorSender {
    type Error = <v2::Framing as ii_wire::Framing>::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.project()
            .inner
            .poll_ready(cx)
            .map_err(|e| ii_stratum::error::ErrorKind::Io(e.to_string()).into())
    }

    fn start_send(
        self: Pin<&mut Self>,
        item: <v2::Framing as ii_wire::Framing>::Tx,
    ) -> Result<(), Self::Error> {
        self.project()
            .inner
            .start_send(item)
            .map_err(|e| ii_stratum::error::ErrorKind::Io(e.to_string()).into())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.project()
            .inner
            .poll_flush(cx)
            .map_err(|e| ii_stratum::error::ErrorKind::Io(e.to_string()).into())
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.project()
            .inner
            .poll_close(cx)
            .map_err(|e| ii_stratum::error::ErrorKind::Io(e.to_string()).into())
    }
}

/// Helper wrapper that wraps every item into a Result compatible with
/// v2::DynFramedStream
#[pin_project]
#[derive(Debug)]
struct V2ConnectorReceiver {
    #[pin]
    inner: mpsc::Receiver<v2::Frame>,
}

impl V2ConnectorReceiver {
    fn new(inner: mpsc::Receiver<v2::Frame>) -> Self {
        Self { inner }
    }
}

impl Stream for V2ConnectorReceiver {
    type Item =
        Result<<v2::Framing as ii_wire::Framing>::Tx, <v2::Framing as ii_wire::Framing>::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        // Wrap every value into Ok as the incoming items from the channel are never marked with
        // an error
        self.project()
            .inner
            .poll_next(cx)
            .map(|value| value.map(Ok))
    }
}

/// This object receives V1 messages and passes them to `V2ToV1Translation` component for
/// translation. The user of this component is provided with an Rx/Tx channel pair that is
/// intended for sending V2 messages and receiving the translated V2 messages.
/// V1 messages received from the translator are sent out via V1 connection.
struct TranslationHandler {
    /// Actual protocol translator
    translation: V2ToV1Translation,
    /// Upstream V1 connection
    v1_conn: v1::Framed,
    /// Receiver for V1 frames from the translator that will be sent out via V1 connection
    v1_translation_receiver: mpsc::Receiver<v1::Frame>,
    /// V2 Frames from the client that we use for feeding the translator
    v2_client_receiver: mpsc::Receiver<v2::Frame>,
}

impl TranslationHandler {
    const MAX_TRANSLATION_CHANNEL_SIZE: usize = 10;

    /// Builds the new translation handler and provides Tx/Rx communication ends
    fn new(
        v1_conn: v1::Framed,
        options: V2ToV1TranslationOptions,
    ) -> (Self, mpsc::Receiver<v2::Frame>, mpsc::Sender<v2::Frame>) {
        let (v1_translation_sender, v1_translation_receiver) =
            mpsc::channel(Self::MAX_TRANSLATION_CHANNEL_SIZE);
        let (v2_translation_sender, v2_translation_receiver) =
            mpsc::channel(Self::MAX_TRANSLATION_CHANNEL_SIZE);
        let (v2_client_sender, v2_client_receiver) =
            mpsc::channel(Self::MAX_TRANSLATION_CHANNEL_SIZE);

        let translation =
            V2ToV1Translation::new(v1_translation_sender, v2_translation_sender, options);

        (
            Self {
                translation,
                v1_conn,
                v1_translation_receiver,
                v2_client_receiver,
            },
            v2_translation_receiver,
            v2_client_sender,
        )
    }

    /// Executive part of the translation handler that drives the translation component and acts
    /// like a message pump between the actual V2 client, translation component and upstream V1
    /// server.
    /// The main task selects from the following events and performs corresponding acction
    /// - v1_conn -> build message + accept(translation)
    /// - v2_client_receiver -> build message + accept(translation)
    /// - v1_translation_receiver -> send
    /// terminate upon any error or timeout
    async fn run(mut self) -> error::Result<()> {
        //while !self.status.is_shutting_down() {
        trace!("Starting V2->V1 translation handler");
        loop {
            select! {
                // Receive V1 frame and translate it to V2 message
                v1_frame = self.v1_conn.next().timeout(super::super::StratumClient::EVENT_TIMEOUT)
                .fuse()
                => {
                    match v1_frame {
                        Ok(Some(v1_frame)) => {
                            let v1_msg = v1::build_message_from_frame(v1_frame?)?;
                            v1_msg.accept(&mut self.translation).await;
                        }
                        Ok(None) | Err(_) => {
                            Err("Upstream V1 stratum connection dropped terminating translation")?;
                        }
                    }
                },
                // Receive V2 frame from our client (no timeout needed) and pass it to V1
                // translation
                v2_frame = self.v2_client_receiver.next().fuse() => {
                    match v2_frame {
                        Some(v2_frame) => {
                            let v2_msg = v2::build_message_from_frame(v2_frame)?;
                            v2_msg.accept(&mut self.translation).await;
                        }
                        None => {
                            Err("V2 client shutdown, terminating translation")?;
                        }
                    }
                },
                // Receive V1 frame from the translation and send it upstream
                v1_frame = self.v1_translation_receiver.next().fuse() => {
                    match v1_frame {
                        Some(v1_frame) => self
                            .v1_conn
                            .send(v1_frame)
                            // NOTE: this timeout is important otherwise the whole task could
                            // block indefinitely and the above timeout for v1_conn wouldn't
                            // do anything. Besides this, we don't want to wait with system time
                            // out in case the upstream connection just hangs
                            .timeout(super::super::StratumClient::EVENT_TIMEOUT)
                            .await
                            // Unwrap timeout and actual sending error
                            .map_err(|e| "V1 send timeout")??,
                        None => {
                            Err("V1 translation component terminated, terminating translation")?;
                        }
                    }
                },
            }
        }
    }
}
