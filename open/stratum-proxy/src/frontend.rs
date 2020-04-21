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

use std::convert::TryFrom;
use std::path::PathBuf;
use structopt::StructOpt;
use tokio::{fs::File, io::AsyncReadExt};

use ii_async_compat::tokio;
use ii_stratum::v2;
use ii_wire::Address;

use crate::error::{Error, Result};

#[derive(StructOpt, Debug)]
#[structopt(name = "stratum-proxy", about = "Stratum V2->V1 translating proxy.")]
pub struct Args {
    /// Listen address
    #[structopt(
        short = "l",
        long = "listen",
        default_value = "localhost:3336",
        help = "Address to listen on for incoming Stratum V2 connections"
    )]
    pub listen_address: Address,

    /// Remote V1 endpoint where to connect to
    #[structopt(
        short = "u",
        long = "v1-upstream",
        name = "HOSTNAME:PORT",
        help = "Address of the upstream Stratum V1 server that the proxy connects to"
    )]
    pub upstream_address: Address,

    #[structopt(
        long,
        help = "Disable noise protocol handshake, all services will be provided unencrypted"
    )]
    pub insecure: bool,

    /// Certificate file
    #[structopt(short = "c", long, parse(from_os_str), required_unless("insecure"))]
    pub certificate_file: Option<PathBuf>,

    /// Secret key as counter part of the public key in the configured public certificate
    #[structopt(short = "s", long, parse(from_os_str), required_unless("insecure"))]
    pub secret_key_file: Option<PathBuf>,
}

impl Args {
    /// Optionally read certificate and secret keypair
    /// Return:
    ///  - None - when `insecure` is true
    ///  - build `Certificate` + `StaticSecretKeyFormat` pair otherwise
    pub async fn read_certificate_secret_key_pair(
        &self,
    ) -> Result<
        Option<(
            v2::noise::auth::Certificate,
            v2::noise::auth::StaticSecretKeyFormat,
        )>,
    > {
        let certificate_key_pair = match self.insecure {
            true => None,
            false => {
                let certificate = read_from_file::<v2::noise::auth::Certificate>(
                    self.certificate_file.as_ref(),
                    "Certificate",
                )
                .await?;
                let secret_key = read_from_file::<v2::noise::auth::StaticSecretKeyFormat>(
                    self.secret_key_file.as_ref(),
                    "Secret key",
                )
                .await?;
                Some((certificate, secret_key))
            }
        };

        Ok(certificate_key_pair)
    }
}

pub async fn read_from_file<T>(
    file_path_buf: Option<&PathBuf>,
    error_context_descr: &str,
) -> Result<T>
where
    T: TryFrom<String>,
    <T as TryFrom<String>>::Error: std::error::Error + Send + Sync + 'static,
{
    let file_path_buf = file_path_buf.expect(&format!("BUG: missing path {}", error_context_descr));

    let mut file = File::open(file_path_buf).await?;
    let mut file_content = String::new();
    file.read_to_string(&mut file_content).await?;

    let parsed_file_content = T::try_from(file_content)
        .map_err(|e| Error::InvalidFile(format!("Error: {} in file {:?}", e, file_path_buf)))?;

    Ok(parsed_file_content)
}
