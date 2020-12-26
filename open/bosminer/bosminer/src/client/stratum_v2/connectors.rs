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

//! Provides basic types for protocol connector abstraction

pub mod insecure;
pub mod noise;
pub mod v1;

use std::pin::Pin;
use tokio::net::TcpStream;

use ii_async_compat::prelude::*;
use ii_stratum::v2::{DynFramedSink, DynFramedStream};

use crate::error;

/// Type of the future that builds a connection using a particular connector
type DynConnectFuture =
    Pin<Box<dyn Future<Output = error::Result<(DynFramedSink, DynFramedStream)>> + Send + 'static>>;

/// Type for closure that can build the above future
pub(crate) type DynConnectFn = Box<dyn Fn(TcpStream) -> DynConnectFuture + Send + Sync + 'static>;

//struct Connector {
//    connector_future: ConnectorFuture,
//}
//
//impl Connector {
//    fn new(connector_future: ConnectorFuture) -> Self {
//        Self {
//            connector_future,
//        }
//    }
//
//    async fn connect(self, connection: TcpStream) -> error::Result<(DynFramedSink,
//                                                                   DynFramedStream)> {
//
//    }
//}
