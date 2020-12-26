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

//! Defines the API command handler (`Handler`)

use crate::response;
use crate::support::ValueExt as _;
use crate::support::{MultiResponse, ResponseType, UnixTime, When};

use serde_json as json;

use ii_async_compat::futures::Future;

use std::collections::HashMap;
use std::marker;
use std::pin::Pin;
use std::sync::Arc;

/// List of all supported commands.
const POOLS: &str = "pools";
const DEVS: &str = "devs";
const EDEVS: &str = "edevs";
const SUMMARY: &str = "summary";
const VERSION: &str = "version";
const SWITCH_POOL: &str = "switchpool";
const CONFIG: &str = "config";
const ENABLE_POOL: &str = "enablepool";
const DISABLE_POOL: &str = "disablepool";
const ADD_POOL: &str = "addpool";
const REMOVE_POOL: &str = "removepool";
const STATS: &str = "stats";
const ESTATS: &str = "estats";
const CHECK: &str = "check";
const COIN: &str = "coin";
const ASC_COUNT: &str = "asccount";
const ASC: &str = "asc";
const LCD: &str = "lcd";

// List of all standard commands which can be optionally implemented.
pub const DEVDETAILS: &str = "devdetails";

// List of all extended commands which have to be implemented externally.
pub const TEMPCTRL: &str = "tempctrl";
pub const TEMPS: &str = "temps";
pub const FANS: &str = "fans";
pub const TUNERSTATUS: &str = "tunerstatus";

pub type Result<T> = std::result::Result<T, response::Error>;
/// Type describing command table
pub type Map = HashMap<&'static str, Descriptor>;

/// A handler to be implemented by the API implementation,
/// takes care of producing a response for each command.
#[async_trait::async_trait]
pub trait Handler: Send + Sync {
    async fn handle_pools(&self) -> Result<response::Pools>;
    async fn handle_devs(&self) -> Result<response::Devs>;
    async fn handle_edevs(&self) -> Result<response::Devs>;
    async fn handle_summary(&self) -> Result<response::Summary>;
    async fn handle_switch_pool(
        &self,
        parameter: Option<&json::Value>,
    ) -> Result<response::SwitchPool>;
    async fn handle_config(&self) -> Result<response::Config>;
    async fn handle_add_pool(&self, parameter: Option<&json::Value>) -> Result<response::AddPool>;
    async fn handle_enable_pool(
        &self,
        parameter: Option<&json::Value>,
    ) -> Result<response::EnablePool>;
    async fn handle_disable_pool(
        &self,
        parameter: Option<&json::Value>,
    ) -> Result<response::DisablePool>;
    async fn handle_remove_pool(
        &self,
        parameter: Option<&json::Value>,
    ) -> Result<response::RemovePool>;
    async fn handle_stats(&self) -> Result<response::Stats>;
    async fn handle_estats(&self) -> Result<response::Stats>;
    async fn handle_coin(&self) -> Result<response::Coin>;
    async fn handle_asc_count(&self) -> Result<response::AscCount>;
    async fn handle_asc(&self, parameter: Option<&json::Value>) -> Result<response::Asc>;
    async fn handle_lcd(&self) -> Result<response::Lcd>;
}

/// Holds an incoming API command
pub struct Request {
    value: json::Value,
}

impl Request {
    pub fn new(value: json::Value) -> Self {
        Self { value }
    }
}

pub type AsyncHandler = Pin<Box<dyn Future<Output = Result<response::Dispatch>> + Send + 'static>>;

pub type ParameterLessHandler = Box<dyn Fn() -> AsyncHandler + Send + Sync>;
pub type ParameterHandler = Box<dyn Fn(Option<&json::Value>) -> AsyncHandler + Send + Sync>;

pub type ParameterCheckHandler =
    Box<dyn Fn(&str, &Option<&json::Value>) -> Result<()> + Send + Sync>;

pub enum HandlerType {
    ParameterLess(ParameterLessHandler),
    Parameter(ParameterHandler),
    Version,
    Check,
}

impl HandlerType {
    pub fn has_parameters(&self) -> bool {
        match self {
            HandlerType::ParameterLess(_) => false,
            HandlerType::Parameter(_) => true,
            HandlerType::Version => false,
            HandlerType::Check => true,
        }
    }
}

/// Describes individual commands and async handler associated with this command
pub struct Descriptor {
    handler: HandlerType,
    parameter_check: Option<ParameterCheckHandler>,
}

impl Descriptor {
    pub fn new<T>(_name: &'static str, handler: HandlerType, parameter_check: T) -> Self
    where
        T: Into<Option<ParameterCheckHandler>>,
    {
        Self {
            handler,
            parameter_check: parameter_check.into(),
        }
    }

    #[inline]
    pub fn has_parameters(&self) -> bool {
        self.handler.has_parameters()
    }
}

/// Generates a descriptor for a specified command type (`ParameterLess` or `Parameter`) that also
/// contains an appropriate handler
#[macro_export]
macro_rules! command {
    ($name:ident: ParameterLess -> $handler:ident . $method:ident) => {{
        let handler = $handler.clone();
        let f: $crate::command::ParameterLessHandler = Box::new(move || {
            let handler = handler.clone();
            Box::pin(async move { handler.$method().await.map(|response| response.into()) })
        });
        let handler = $crate::command::HandlerType::ParameterLess(f);
        $crate::command::Descriptor::new($name, handler, None)
    }};
    ($name:ident: Parameter($check:expr) -> $handler:ident . $method:ident) => {{
        let handler = $handler.clone();
        let f: $crate::command::ParameterHandler = Box::new(move |parameter| {
            let handler = handler.clone();
            let parameter = parameter.cloned();
            Box::pin(async move {
                handler
                    .$method(parameter.as_ref())
                    .await
                    .map(|response| response.into())
            })
        });
        let handler = $crate::command::HandlerType::Parameter(f);
        $crate::command::Descriptor::new($name, handler, $check)
    }};
    ($name:ident: BuiltIn($type:ident)) => {
        $crate::command::Descriptor::new($name, $crate::command::HandlerType::$type, None)
    };
}

/// Generates a map that associated a command name with its descriptor
#[macro_export]
macro_rules! commands {
    () => (
        $crate::command::Map::new()
    );
    ($(($name:ident: $type:ident$(($parameter:ident))? $(-> $handler:ident . $method:ident)?)),+) => {
        {
            let mut map = $crate::command::Map::new();
            $(
                let descriptor = command!($name: $type $(($parameter))? $(-> $handler . $method)?);
                map.insert($name, descriptor);
            )*
            map
        }
    }
}

/// Generic command receiving and processing object that dispatches command handling
/// user provided handler methods.
pub struct Receiver<T = UnixTime> {
    commands: Map,
    miner_signature: String,
    miner_version: String,
    description: String,
    _marker: marker::PhantomData<T>,
}

impl<T> Receiver<T>
where
    T: When,
{
    /// Builds a new command receiver that delegates processing of all standard commands to the
    /// provided `handler`. Optional `custom_commands` must be convertible to a `command::Map` and
    /// extend the command map created for the basic commands.
    pub fn new<U, V>(
        handler: U,
        miner_signature: String,
        miner_version: String,
        custom_commands: V,
    ) -> Self
    where
        U: Handler + 'static,
        V: Into<Option<Map>>,
    {
        let handler = Arc::new(handler);

        let check_switch_pool: ParameterCheckHandler =
            Box::new(|command, parameter| Self::check_pool_id(command, parameter));
        let check_enable_pool: ParameterCheckHandler =
            Box::new(|command, parameter| Self::check_pool_id(command, parameter));
        let check_disable_pool: ParameterCheckHandler =
            Box::new(|command, parameter| Self::check_pool_id(command, parameter));
        let check_add_pool: ParameterCheckHandler =
            Box::new(|command, parameter| Self::check_add_pool(command, parameter));
        let check_remove_pool: ParameterCheckHandler =
            Box::new(|command, parameter| Self::check_pool_id(command, parameter));
        let check_asc: ParameterCheckHandler =
            Box::new(|command, parameter| Self::check_asc(command, parameter));

        let mut commands = commands![
            // generic commands
            (POOLS: ParameterLess -> handler.handle_pools),
            (DEVS: ParameterLess -> handler.handle_devs),
            (EDEVS: ParameterLess -> handler.handle_edevs),
            (SUMMARY: ParameterLess -> handler.handle_summary),
            (SWITCH_POOL: Parameter(check_switch_pool) -> handler.handle_switch_pool),
            (CONFIG: ParameterLess -> handler.handle_config),
            (ENABLE_POOL: Parameter(check_enable_pool) -> handler.handle_enable_pool),
            (DISABLE_POOL: Parameter(check_disable_pool) -> handler.handle_disable_pool),
            (ADD_POOL: Parameter(check_add_pool) -> handler.handle_add_pool),
            (REMOVE_POOL: Parameter(check_remove_pool) -> handler.handle_remove_pool),
            (STATS: ParameterLess -> handler.handle_stats),
            (ESTATS: ParameterLess -> handler.handle_estats),
            (COIN: ParameterLess -> handler.handle_coin),
            (ASC_COUNT: ParameterLess -> handler.handle_asc_count),
            (ASC: Parameter(check_asc) -> handler.handle_asc),
            (LCD: ParameterLess -> handler.handle_lcd),
            // special built-in commands
            (VERSION: BuiltIn(Version)),
            (CHECK: BuiltIn(Check))
        ];

        if let Some(custom_commands) = custom_commands.into() {
            commands.extend(custom_commands.into_iter());
        }

        let description = format!("{} {}", miner_signature.clone(), miner_version.clone());
        Self {
            commands,
            miner_signature,
            miner_version,
            description,
            _marker: marker::PhantomData,
        }
    }

    fn check_add_pool(_command: &str, parameter: &Option<&json::Value>) -> Result<()> {
        const ARG_COUNT: usize = 3;
        match parameter {
            Some(json::Value::String(value)) => {
                if value.splitn(ARG_COUNT, super::PARAMETER_DELIMITER).count() == ARG_COUNT {
                    Ok(())
                } else {
                    Err(response::ErrorCode::InvalidAddPoolDetails(value.clone()).into())
                }
            }
            Some(json::Value::Number(value)) => {
                Err(response::ErrorCode::InvalidAddPoolDetails(value.to_string()).into())
            }
            // CGMiner recognizes strings and integers as the same type. Other types (array, map,
            // ..) are reported as a missing parameter. Therefore, we match anything else as
            // missing parameter.
            _ => Err(response::ErrorCode::MissingAddPoolDetails.into()),
        }
    }

    fn check_pool_id(_command: &str, parameter: &Option<&json::Value>) -> Result<()> {
        match parameter {
            Some(value) if value.is_i32() => Ok(()),
            _ => Err(response::ErrorCode::MissingPoolParameter.into()),
        }
    }

    fn check_asc(_command: &str, parameter: &Option<&json::Value>) -> Result<()> {
        match parameter {
            Some(value) if value.is_i32() => Ok(()),
            _ => Err(response::ErrorCode::MissingAscParameter.into()),
        }
    }

    fn handle_version(&self) -> Result<response::Version> {
        Ok(response::Version {
            signature: self.miner_signature.to_string(),
            miner: self.miner_version.to_string(),
            api: crate::API_VERSION.to_string(),
        })
    }

    fn handle_check(&self, parameter: Option<&json::Value>) -> Result<response::Check> {
        let command =
            parameter.ok_or_else(|| response::Error::from(response::ErrorCode::MissingCheckCmd))?;
        let result = match command {
            json::Value::String(command) => self.commands.get(command.as_str()).into(),
            _ => response::Bool::N,
        };

        Ok(response::Check {
            exists: result,
            access: result,
        })
    }

    /// Handles a single `command` with optional `parameter`. `multi_command` flag ensures that no
    /// command with parameters can be processed in batched mode.
    async fn handle_single(
        &self,
        command: &str,
        parameter: Option<&json::Value>,
        multi_command: bool,
    ) -> response::Dispatch {
        let dispatch = match self.commands.get(command) {
            Some(descriptor) => {
                if multi_command && descriptor.has_parameters() {
                    Err(response::ErrorCode::AccessDeniedCmd(command.to_string()).into())
                } else {
                    let check_result = descriptor
                        .parameter_check
                        .as_ref()
                        .map_or(Ok(()), |check| check(command, &parameter));
                    match check_result {
                        Ok(_) => match &descriptor.handler {
                            HandlerType::ParameterLess(handle) => handle().await,
                            HandlerType::Parameter(handle) => handle(parameter).await,
                            HandlerType::Version => {
                                self.handle_version().map(|response| response.into())
                            }
                            HandlerType::Check => {
                                self.handle_check(parameter).map(|response| response.into())
                            }
                        },
                        Err(response) => Err(response),
                    }
                }
            }
            None => Err(response::ErrorCode::InvalidCommand.into()),
        };

        dispatch.unwrap_or_else(|error| error.into())
    }

    #[inline]
    fn get_single_response(&self, dispatch: response::Dispatch) -> ResponseType {
        ResponseType::Single(dispatch.into_response(
            T::when(),
            &self.miner_signature,
            &self.description,
        ))
    }

    pub fn error_response(&self, error_code: response::ErrorCode) -> ResponseType {
        self.get_single_response(error_code.into())
    }

    /// Handles a command request that can actually be a batched request of multiple commands
    pub async fn handle(&self, command_request: Request) -> ResponseType {
        let command = match command_request
            .value
            .get("command")
            .and_then(json::Value::as_str)
        {
            None => return self.get_single_response(response::ErrorCode::MissingCommand.into()),
            Some(value) => value,
        };
        let commands: Vec<_> = command
            .split('+')
            .filter(|command| command.len() > 0)
            .collect();
        let parameter = command_request.value.get("parameter");

        if commands.len() == 0 {
            self.get_single_response(response::ErrorCode::InvalidCommand.into())
        } else if commands.len() == 1 {
            self.get_single_response(self.handle_single(command, parameter, false).await)
        } else {
            let mut responses = MultiResponse::new();
            for command in commands {
                if let ResponseType::Single(response) =
                    self.get_single_response(self.handle_single(command, parameter, true).await)
                {
                    responses.add_response(command, response);
                }
            }
            ResponseType::Multi(responses)
        }
    }
}
