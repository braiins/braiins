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

use std::net::TcpListener as StdTcpListener;
use std::net::ToSocketAddrs as StdToSocketAddrs;
use std::pin::Pin;
use std::task::{Context, Poll};

use ii_async_compat::prelude::*;
use pin_project::pin_project;
use tokio::net::{TcpListener, TcpStream};

#[pin_project]
#[derive(Debug)]
pub struct Server {
    #[pin]
    tcp: TcpListener,
}

impl Server {
    pub fn bind<A: StdToSocketAddrs>(addr: A) -> std::io::Result<Self> {
        let tcp = StdTcpListener::bind(addr)?;
        let tcp = TcpListener::from_std(tcp)?;

        Ok(Server { tcp })
    }
}

impl Stream for Server {
    type Item = std::io::Result<TcpStream>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut tcp = self.project().tcp;

        Pin::new(&mut tcp.incoming()).poll_next(cx)
    }
}
