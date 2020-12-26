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

// Sub-modules with client implementation
pub mod connectors;
pub mod telemetry;

use ii_logging::macros::*;

use crate::error;
use crate::hal;
use crate::job;
use crate::node::{self, Client};
use crate::stats;
use crate::sync;
use crate::work;

use failure::ResultExt;

use ii_bitcoin::HashTrait;

use bosminer_config::{ClientDescriptor, ClientProtocol};
use bosminer_macros::ClientNode;

use async_trait::async_trait;
use futures::channel::mpsc;
use futures::lock::Mutex;
use ii_async_compat::prelude::*;
use ii_async_compat::select;

use std::collections::VecDeque;
use std::fmt;
use std::str::FromStr;
use std::sync::Mutex as StdMutex;
use std::sync::{Arc, Weak};
use std::time;

use ii_stratum::v2::messages::{
    NewMiningJob, OpenStandardMiningChannel, OpenStandardMiningChannelError,
    OpenStandardMiningChannelSuccess, Reconnect, SetNewPrevHash, SetTarget, SetupConnection,
    SetupConnectionError, SetupConnectionSuccess, SubmitSharesError, SubmitSharesStandard,
    SubmitSharesSuccess,
};
use ii_stratum::v2::types::*;
use ii_stratum::v2::{build_message_from_frame, extensions, Handler};
use ii_stratum::v2::{
    framing::{Framing, Header},
    DynFramedSink, DynFramedStream,
};

use std::collections::HashMap;

// TODO: move it to the stratum crate
const VERSION_MASK: u32 = 0x1fffe000;

#[derive(Debug, Clone)]
pub struct ConnectionDetails {
    /// Protocol provides valuable diagnostic information as stratum_v2 operates in multiple
    /// modes including V2->V1 translation
    pub protocol: ClientProtocol,
    pub user: String,
    pub host: String,
    pub port: u16,
}

impl ConnectionDetails {
    pub fn from_descriptor(descriptor: &ClientDescriptor) -> Self {
        Self {
            protocol: descriptor.protocol.clone(),
            user: descriptor.user.clone(),
            host: descriptor.host.clone(),
            port: descriptor.port(),
        }
    }

    fn get_host_and_port(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Clone)]
pub struct StratumJob {
    client: Weak<StratumClient>,
    id: u32,
    channel_id: u32,
    version: u32,
    prev_hash: ii_bitcoin::DHash,
    merkle_root: ii_bitcoin::DHash,
    time: u32,
    bits: u32,
    target: ii_bitcoin::Target,
}

impl StratumJob {
    pub fn new(
        client: Arc<StratumClient>,
        job_msg: &NewMiningJob,
        prevhash_msg: &SetNewPrevHash,
        target: ii_bitcoin::Target,
    ) -> Self {
        Self {
            client: Arc::downgrade(&client),
            id: job_msg.job_id,
            channel_id: job_msg.channel_id,
            version: job_msg.version,
            prev_hash: ii_bitcoin::DHash::from_slice(prevhash_msg.prev_hash.as_ref())
                .expect("BUG: Stratum: incorrect size of prev hash"),
            merkle_root: ii_bitcoin::DHash::from_slice(job_msg.merkle_root.as_ref())
                .expect("BUG: Stratum: incorrect size of merkle root"),
            time: prevhash_msg.min_ntime,
            bits: prevhash_msg.nbits,
            target,
        }
    }
}

impl job::Bitcoin for StratumJob {
    fn origin(&self) -> Weak<dyn node::Client> {
        self.client.clone()
    }

    fn version(&self) -> u32 {
        self.version
    }

    fn version_mask(&self) -> u32 {
        VERSION_MASK
    }

    fn previous_hash(&self) -> &ii_bitcoin::DHash {
        &self.prev_hash
    }

    fn merkle_root(&self) -> &ii_bitcoin::DHash {
        &self.merkle_root
    }

    fn time(&self) -> u32 {
        self.time
    }

    fn bits(&self) -> u32 {
        self.bits
    }

    fn target(&self) -> ii_bitcoin::Target {
        self.target
    }

    fn is_valid(&self) -> bool {
        // TODO: currently there is no easy way to detect the job is valid -> we have to check
        //  its presence in the registry. The inequality below was possible in the previous
        //  iteration of the protocol
        // self.block_height >= self.current_block_height.load(Ordering::Relaxed)
        true
    }
}

/// Queue that contains pairs of solution and its assigned sequence number. It is our responsibility
/// to keep the sequence number monotonic so that we as a stratum V2 client can easily process bulk
/// acknowledgements. The sequence number type has been selected as u32 to match
/// up with the protocol.
type SolutionQueue = Mutex<VecDeque<(work::Solution, u32)>>;

/// Helper task for `StratumClient` that implements Stratum V2 visitor which processes incoming
/// messages from remote server.
struct StratumEventHandler {
    client: Arc<StratumClient>,
    all_jobs: HashMap<u32, NewMiningJob>,
    current_prevhash_msg: Option<SetNewPrevHash>,
    /// Mining target for the next job that is to be solved
    current_target: ii_bitcoin::Target,
}

impl StratumEventHandler {
    pub fn new(client: Arc<StratumClient>, current_target: ii_bitcoin::Target) -> Self {
        Self {
            client,
            all_jobs: Default::default(),
            current_prevhash_msg: None,
            current_target,
        }
    }

    /// Convert new mining job message into StratumJob and send it down the line for solving.
    ///
    /// * `job_msg` - job message used as a base for the StratumJob
    async fn update_job(&mut self, job_msg: &NewMiningJob) {
        let job = Arc::new(StratumJob::new(
            self.client.clone(),
            job_msg,
            self.current_prevhash_msg
                .as_ref()
                .expect("TODO: no prevhash"),
            self.current_target,
        ));
        self.client.update_last_job(job.clone()).await;
        self.client.job_sender.lock().await.send(job);
    }

    fn update_target(&mut self, value: Uint256Bytes) {
        let new_target: ii_bitcoin::Target = value.into();
        info!(
            "Stratum: changing target to {} diff={}",
            new_target,
            new_target.get_difficulty()
        );
        self.current_target = new_target;
    }

    async fn process_accepted_shares(&self, success_msg: &SubmitSharesSuccess) {
        let now = std::time::Instant::now();
        while let Some((solution, seq_num)) = self.client.solutions.lock().await.pop_front() {
            info!(
                "Stratum: accepted solution #{} with nonce={:08x}",
                seq_num,
                solution.nonce()
            );
            self.client
                .client_stats
                .accepted
                .account_solution(&solution.job_target(), now)
                .await;
            if success_msg.last_seq_num == seq_num {
                // all accepted solutions have been found
                return;
            }
        }
        // FIXME: Change trace! to warn! after proper implementation of sequence number
        trace!(
            "Stratum: last accepted solution #{} hasn't been found!",
            success_msg.last_seq_num
        );
    }

    async fn process_rejected_shares(&self, error_msg: &SubmitSharesError) {
        let now = std::time::Instant::now();
        while let Some((solution, seq_num)) = self.client.solutions.lock().await.pop_front() {
            if error_msg.seq_num == seq_num {
                info!(
                    "Stratum: rejected solution #{} with nonce={:08x}!",
                    seq_num,
                    solution.nonce()
                );
                self.client
                    .client_stats
                    .rejected
                    .account_solution(&solution.job_target(), now)
                    .await;
                // the rejected solution has been found
                return;
            } else {
                // TODO: this is currently not according to stratum V2 specification
                // preceding solutions are treated as accepted
                info!(
                    "Stratum: accepted solution #{} with nonce={}",
                    seq_num,
                    solution.nonce()
                );
                self.client
                    .client_stats
                    .accepted
                    .account_solution(&solution.job_target(), now)
                    .await;
                warn!(
                    "Stratum: the solution #{} precedes rejected solution #{}!",
                    seq_num, error_msg.seq_num
                );
                warn!(
                    "Stratum: the solution #{} is treated as an accepted one",
                    seq_num
                );
            }
        }
        warn!(
            "Stratum: rejected solution #{} hasn't been found!",
            error_msg.seq_num
        );
    }
}

#[async_trait]
impl Handler for StratumEventHandler {
    // The rules for prevhash/mining job pairing are (currently) as follows:
    //  - when mining job comes
    //      - store it (by id)
    //      - start mining it if it doesn't have the future_job flag set
    //  - when prevhash message comes
    //      - replace it
    //      - start mining the job it references (by job id)
    //      - flush all other jobs

    async fn visit_new_mining_job(&mut self, _header: &Header, job_msg: &NewMiningJob) {
        // all jobs since last `prevmsg` have to be stored in job table
        self.all_jobs.insert(job_msg.job_id, job_msg.clone());
        // TODO: close connection when maximal capacity of `all_jobs` has been reached

        // When not marked as future job, we can start mining on it right away
        // TODO see the _channels variant when consolidating this version of the client.
        //  Transform the documentation from there too. Currently, this workaround with
        //  .is_some() prevents a problem when a server indicates a new job however it doesn't
        //  send the new prevhash ahead of this job. This scenario is still yet to be investigated
        //  as it should prevented typically on the V2->V1->upstream translation proxies. These
        //  proxies should guarantee that no such case like a job without a prevhash would exist.
        if !job_msg.future_job && self.current_prevhash_msg.is_some() {
            self.update_job(job_msg).await;
        }
    }

    async fn visit_set_new_prev_hash(&mut self, _header: &Header, prevhash_msg: &SetNewPrevHash) {
        self.current_prevhash_msg.replace(prevhash_msg.clone());

        // find the future job with ID referenced in prevhash_msg
        let (_, mut future_job_msg) = self
            .all_jobs
            .remove_entry(&prevhash_msg.job_id)
            .expect("TODO: requested job ID not found");

        // remove all other jobs (they are now invalid)
        self.all_jobs.retain(|_, _| true);
        // turn the job into an immediate job
        future_job_msg.future_job = false;
        // reinsert the job
        self.all_jobs
            .insert(future_job_msg.job_id, future_job_msg.clone());

        // and start immediately solving it
        self.update_job(&future_job_msg).await;
    }

    async fn visit_reconnect(&mut self, _header: &Header, reconnect_msg: &Reconnect) {
        let new_host: String = reconnect_msg.new_host.clone().into();
        info!(
            "Reconnect to: {} {}, received on established connection: {}",
            new_host,
            reconnect_msg.new_port,
            self.client.connection_details().get_host_and_port()
        );
        self.client
            .reconnect(
                reconnect_msg.new_host.clone().into(),
                reconnect_msg.new_port,
            )
            .await;
    }

    async fn visit_set_target(&mut self, _header: &Header, target_msg: &SetTarget) {
        self.update_target(target_msg.max_target);
    }

    async fn visit_submit_shares_success(
        &mut self,
        _header: &Header,
        success_msg: &SubmitSharesSuccess,
    ) {
        self.process_accepted_shares(success_msg).await;
    }

    async fn visit_submit_shares_error(&mut self, _header: &Header, error_msg: &SubmitSharesError) {
        self.process_rejected_shares(error_msg).await;
    }
}

struct StratumSolutionHandler {
    client: Arc<StratumClient>,
    connection_tx: Arc<Mutex<DynFramedSink>>,
    seq_num: u32,
}

impl StratumSolutionHandler {
    fn new(client: Arc<StratumClient>, connection_tx: Arc<Mutex<DynFramedSink>>) -> Self {
        Self {
            client,
            connection_tx,
            seq_num: 0,
        }
    }

    async fn process_solution(&mut self, solution: work::Solution) -> error::Result<()> {
        let job: &StratumJob = solution.job();

        let seq_num = self.seq_num;
        self.seq_num = self.seq_num.wrapping_add(1);

        let share_msg = SubmitSharesStandard {
            channel_id: job.channel_id,
            seq_num,
            job_id: job.id,
            nonce: solution.nonce(),
            ntime: solution.time(),
            version: solution.version(),
        };
        // store solution with sequence number for future server acknowledge
        self.client
            .solutions
            .lock()
            .await
            .push_back((solution, seq_num));
        // send solutions back to the stratum server
        StratumClient::send_msg(&self.connection_tx, share_msg)
            .await
            .context("Cannot send submit to stratum server")?;
        // the response is handled in a separate task
        Ok(())
    }
}

struct StratumConnectionHandler {
    client: Arc<StratumClient>,
    init_target: ii_bitcoin::Target,
    status: Option<error::Result<()>>,
}

impl StratumConnectionHandler {
    pub fn new(client: Arc<StratumClient>) -> Self {
        Self {
            client,
            init_target: Default::default(),
            status: None,
        }
    }

    async fn setup_mining_connection(
        &mut self,
        connection_rx: &mut DynFramedStream,
        connection_tx: Arc<Mutex<DynFramedSink>>,
    ) -> error::Result<()> {
        let connection_details = self.client.connection_details();
        let setup_msg = SetupConnection {
            protocol: 0,
            max_version: 2,
            min_version: 2,
            flags: 0,
            endpoint_host: Str0_255::from_string(connection_details.host.clone()),
            endpoint_port: connection_details.port,
            device: self.client.backend_info.clone().unwrap_or_default().into(),
        };
        StratumClient::send_msg(&connection_tx, setup_msg)
            .await
            .context("Cannot send stratum setup mining connection")?;
        let frame = connection_rx
            .next()
            .await
            .ok_or("The remote stratum server was disconnected prematurely")??;
        let response_msg = build_message_from_frame(frame)?;

        self.status = None;
        response_msg.accept(self).await;
        self.status.take().unwrap_or(Err(
            "Unexpected response for stratum setup mining connection".into(),
        ))
    }

    async fn open_channel(
        &mut self,
        connection_rx: &mut DynFramedStream,
        connection_tx: Arc<Mutex<DynFramedSink>>,
    ) -> error::Result<()> {
        let channel_msg = OpenStandardMiningChannel {
            req_id: 10, // TODO? come up with request ID sequencing
            user: self
                .client
                .connection_details()
                .user
                .clone()
                .try_into()
                .expect("BUG: cannot convert 'OpenStandardMiningChannel::user'"),
            nominal_hashrate: 1e9,
            // Maximum bitcoin target is 0xffff << 208 (= difficulty 1 share)
            max_target: ii_bitcoin::Target::default().into(),
        };

        StratumClient::send_msg(&connection_tx, channel_msg)
            .await
            .context("Cannot send stratum open channel")?;
        let frame = connection_rx
            .next()
            .await
            .ok_or("The remote stratum server was disconnected prematurely")??;
        let response_msg = build_message_from_frame(frame)?;

        self.status = None;
        response_msg.accept(self).await;
        self.status.take().unwrap_or(Err(format!(
            "Unexpected response for stratum open channel: {:?}",
            response_msg.header
        )
        .as_str()
        .into()))
    }

    async fn connect(&self) -> error::Result<(DynFramedSink, DynFramedStream)> {
        let connection_details = self.client.connection_details();
        let addr = ii_wire::Address::from_str(connection_details.get_host_and_port().as_str())?;
        let mut client = ii_wire::Client::new(addr);
        // Attempt only once to connect (as the stratum client is being managed externally)
        let connection = client.next().await?;

        (self.client.get_connector)(connection).await
    }

    /// Starts mining session and provides the initial target negotiated by the upstream endpoint
    async fn init_mining_session(
        mut self,
        connection_rx: &mut DynFramedStream,
        connection_tx: Arc<Mutex<DynFramedSink>>,
    ) -> error::Result<ii_bitcoin::Target> {
        self.setup_mining_connection(connection_rx, connection_tx.clone())
            .await
            .context("Cannot setup stratum mining connection")?;
        self.open_channel(connection_rx, connection_tx)
            .await
            .context("Cannot open stratum channel")?;

        Ok(self.init_target)
    }
}

#[async_trait]
impl Handler for StratumConnectionHandler {
    async fn visit_setup_connection_success(
        &mut self,
        _header: &Header,
        _success_msg: &SetupConnectionSuccess,
    ) {
        self.status = Ok(()).into();
    }

    async fn visit_setup_connection_error(
        &mut self,
        _header: &Header,
        error_msg: &SetupConnectionError,
    ) {
        self.status =
            Err(format!("Setup connection error: {}", error_msg.code.to_string()).into()).into();
    }

    async fn visit_open_standard_mining_channel_success(
        &mut self,
        _header: &Header,
        success_msg: &OpenStandardMiningChannelSuccess,
    ) {
        self.init_target = success_msg.target.into();
        self.status = Ok(()).into();
    }

    async fn visit_open_standard_mining_channel_error(
        &mut self,
        _header: &Header,
        error_msg: &OpenStandardMiningChannelError,
    ) {
        self.status =
            Err(format!("Open channel error: {}", error_msg.code.to_string()).into()).into();
    }

    async fn visit_reconnect(&mut self, _header: &Header, reconnect_msg: &Reconnect) {
        let new_host: String = reconnect_msg.new_host.clone().into();
        info!(
            "Reconnect to: {} {}, received while connecting to: {}",
            new_host,
            reconnect_msg.new_port,
            self.client.connection_details().get_host_and_port()
        );
        self.client
            .reconnect(new_host, reconnect_msg.new_port)
            .await;
    }
}

/// Messages to control the extension channel
#[derive(Debug)]
pub enum ExtensionChannelMsg {
    /// Starts the extension channel
    Start,
    /// Stops the extension channel
    Stop,
    /// Frame being forwarded into the extension channel
    Frame(<Framing as ii_wire::Framing>::Tx),
}

/// Receiver for Stratum <-- Remote direction (stratum client end)
pub type ExtensionChannelToStratumReceiver = mpsc::Receiver<<Framing as ii_wire::Framing>::Rx>;
/// Remote sender for Stratum <-- Remote direction (remote end)
pub type ExtensionChannelToStratumSender = mpsc::Sender<<Framing as ii_wire::Framing>::Tx>;

/// Receiver for Stratum --> Remote direction (remote end)
pub type ExtensionChannelFromStratumReceiver = mpsc::Receiver<ExtensionChannelMsg>;
/// Sender for Stratum --> Remote direction (stratum client end)
pub type ExtensionChannelFromStratumSender = mpsc::Sender<ExtensionChannelMsg>;

#[derive(ClientNode)]
pub struct StratumClient {
    connection_details: Arc<StdMutex<ConnectionDetails>>,
    backend_info: Option<hal::BackendInfo>,
    #[member_status]
    status: sync::StatusMonitor,
    #[member_client_stats]
    client_stats: stats::BasicClient,
    stop_sender: mpsc::Sender<()>,
    stop_receiver: Mutex<mpsc::Receiver<()>>,
    // Last job has to be weak reference to prevent circular reference (the `StratumJob` keeps
    // reference to `StratumClient`)
    last_job: Mutex<Option<Arc<StratumJob>>>,
    solutions: SolutionQueue,
    job_sender: Mutex<job::Sender>,
    solution_receiver: Mutex<job::SolutionReceiver>,
    /// Frames received from this channel will be forwarded to the network connection
    extension_channel_receiver: Mutex<ExtensionChannelToStratumReceiver>,
    /// Frames intended for the specified extension will be forwarded into this channel (wrapped
    /// into ExtensionChannelMsg
    extension_channel_sender: Mutex<ExtensionChannelFromStratumSender>,
    // Each protocol scheme provides a dedicated connector. This closure allows building the
    // connector future at a time when the physical TCP connection is established
    get_connector: connectors::DynConnectFn,
}

impl StratumClient {
    const CONNECTION_TIMEOUT: time::Duration = time::Duration::from_secs(5);
    const EVENT_TIMEOUT: time::Duration = time::Duration::from_secs(150);
    const SEND_TIMEOUT: time::Duration = time::Duration::from_secs(2);

    /// Start a task that plays a dummy role for both communication channels that the stratum
    /// client uses to talk to stratum extension.
    fn start_dummy_extension_task(
        connection_details: ConnectionDetails,
    ) -> (
        ExtensionChannelToStratumReceiver,
        ExtensionChannelFromStratumSender,
    ) {
        // Dummy channel for Stratum client --> extension communication
        let (sender_from_client, mut receiver_from_client) = mpsc::channel(1);
        // Dummy channel for Stratum client <-- extension communication
        let (sender_to_client, receiver_to_client) = mpsc::channel(1);

        tokio::spawn(async move {
            trace!(
                "Stratum extension: starting dummy task[{:?}]... ",
                connection_details
            );
            // Make sure the sender is moved inside the dummy task to prevent it from being
            // dropped. Otherwise the receiver_to_client would immediately indicate end of stream
            let _sender_to_client = sender_to_client;
            while let Some(message) = receiver_from_client.next().await {
                trace!(
                    "Stratum extension: dummy task[{:?}] received: {:?},",
                    connection_details,
                    message
                );
            }
            trace!(
                "Stratum extension: dummy task[{:?}] terminated",
                connection_details
            );
        });
        (receiver_to_client, sender_from_client)
    }

    pub fn new(
        connection_details: ConnectionDetails,
        get_connector: connectors::DynConnectFn,
        backend_info: Option<hal::BackendInfo>,
        solver: job::Solver,
        channel: Option<(
            ExtensionChannelToStratumReceiver,
            ExtensionChannelFromStratumSender,
        )>,
    ) -> Self {
        let (stop_sender, stop_receiver) = mpsc::channel(1);

        // Extract the both channel endpoints that connect the client with the stratum extension
        // or populate it with dummy endpoints. That way we can handle the endpoints uniformly
        // regardless whether they are configured or not (see `main_loop()`)
        // that would handle all events regards
        let (extension_channel_receiver, extension_channel_sender) =
            channel.unwrap_or_else(|| Self::start_dummy_extension_task(connection_details.clone()));

        Self {
            connection_details: Arc::new(StdMutex::new(connection_details)),
            get_connector,
            backend_info,
            status: Default::default(),
            client_stats: Default::default(),
            stop_sender: stop_sender,
            stop_receiver: Mutex::new(stop_receiver),
            last_job: Mutex::new(None),
            solutions: Mutex::new(VecDeque::new()),
            job_sender: Mutex::new(solver.job_sender),
            solution_receiver: Mutex::new(solver.solution_receiver),
            extension_channel_receiver: Mutex::new(extension_channel_receiver),
            extension_channel_sender: Mutex::new(extension_channel_sender),
        }
    }

    fn connection_details(&self) -> ConnectionDetails {
        self.connection_details
            .lock()
            .expect("BUG: cannot lock connection details")
            .clone()
    }

    async fn update_last_job(&self, job: Arc<StratumJob>) {
        self.last_job.lock().await.replace(job);
    }

    /// Send a message down a specified Tx Sink
    /// TODO: temporarily, this became an associated method so that we don't have to generalize
    ///  with type parameters the full StratumClient struct. Once this is done, we will use the
    ///  new internal field connection_tx
    async fn send_msg<M>(connection_tx: &Arc<Mutex<DynFramedSink>>, message: M) -> error::Result<()>
    where
        M: TryInto<<Framing as ii_wire::Framing>::Tx, Error = <Framing as ii_wire::Framing>::Error>,
    {
        let frame = message.try_into()?;
        match connection_tx
            .lock()
            .await
            .send(frame)
            .timeout(Self::SEND_TIMEOUT)
            .await
        {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Err("Cannot send message due to timeout")?,
        }
    }

    async fn handle_frame(
        &self,
        frame: <Framing as ii_wire::Framing>::Rx,
        event_handler: &mut StratumEventHandler,
    ) -> error::Result<()> {
        match frame.header.extension_type {
            extensions::BASE => {
                let event_msg = build_message_from_frame(frame)?;
                event_msg.accept(event_handler).await;
            }
            // pass any other extension down the line
            _ => {
                debug!(
                    "Received protocol extension frame: {:x?} passing down",
                    frame
                );
                // Intentionally capture a potential error as an issue with extension channel
                // must not cause the client to fail completely
                if let Err(e) = self
                    .extension_channel_sender
                    .lock()
                    .await
                    .try_send(ExtensionChannelMsg::Frame(frame))
                {
                    debug!(
                        "Cannot pass extension frame, extension channel not available: {:?}",
                        e
                    );
                }
            }
        }
        Ok(())
    }

    async fn main_loop(
        self: Arc<Self>,
        mut connection_rx: DynFramedStream,
        connection_tx: Arc<Mutex<DynFramedSink>>,
        mut event_handler: StratumEventHandler,
    ) -> error::Result<()> {
        let mut solution_receiver = self.solution_receiver.lock().await;
        let mut extension_channel_rx = self.extension_channel_receiver.lock().await;
        let mut solution_handler = StratumSolutionHandler::new(self.clone(), connection_tx.clone());

        // Notify the extension user that we are ready to start forwarding its protocol, use a
        // separate block, so that the lock is dropped immediately after the start notification
        // is sent
        {
            self.extension_channel_sender
                .lock()
                .await
                .try_send(ExtensionChannelMsg::Start)
                .map_err(|e| {
                    info!("Stratum extension channel start error: {:?}", e);
                })
                .expect("BUG: stratum extension channel not available for start");
        }
        while !self.status.is_shutting_down() {
            select! {
                frame = connection_rx.next().timeout(Self::EVENT_TIMEOUT).fuse() => {
                    match frame {
                        Ok(Some(frame)) => self.handle_frame(frame?, &mut event_handler).await?,
                        Ok(None) | Err(_) => {
                            Err("The remote stratum server was disconnected prematurely")?;
                        }
                    }
                }
                // Forward extension protocol frames onto the network
                frame = extension_channel_rx.next().fuse() => {
                    connection_tx.lock().await
                        .send(frame.expect("BUG: extension channel must not shutdown!"))
                        .await?;
                }
                solution = solution_receiver.receive().fuse() => {
                    match solution {
                        Some(solution) => solution_handler.process_solution(solution).await?,
                        None => {
                            // TODO: initiate Destroying and remove error
                            Err("Standard application shutdown")?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn run_job_solver(
        self: Arc<Self>,
        connection_rx: DynFramedStream,
        connection_tx: Arc<Mutex<DynFramedSink>>,
        init_target: ii_bitcoin::Target,
    ) {
        let event_handler = StratumEventHandler::new(self.clone(), init_target);
        // TODO consider changing main_loop to accept Arc<Self> and build the solution_handler
        //  along with solution handler communication channels inside of the main_loop.
        let client = self.clone();
        if let Err(_) = client
            .main_loop(connection_rx, connection_tx, event_handler)
            .await
        {
            self.status.initiate_failing();
        }
    }

    async fn run(self: Arc<Self>) {
        let connection_handler = StratumConnectionHandler::new(self.clone());
        let connection_details = connection_handler.client.connection_details();
        let host_and_port = connection_details.get_host_and_port();
        let user = connection_details.user.clone();

        match connection_handler
            .connect()
            .timeout(Self::CONNECTION_TIMEOUT)
            .await
            .map_err(|_| error::ErrorKind::General("Connection timeout".to_string()).into())
        {
            Ok(Ok((framed_sink, mut framed_stream))) => {
                info!(
                    "Connected {} to: {}",
                    connection_details.protocol,
                    connection_details.get_host_and_port()
                );
                let framed_sink = Arc::new(Mutex::new(framed_sink));
                match connection_handler
                    .init_mining_session(&mut framed_stream, framed_sink.clone())
                    .timeout(Self::CONNECTION_TIMEOUT)
                    .await
                    .map_err(|_| {
                        error::ErrorKind::General("Init mining session timeout".to_string()).into()
                    }) {
                    Ok(Ok(init_target)) => {
                        if self.status.initiate_running() {
                            self.clone()
                                .run_job_solver(framed_stream, framed_sink, init_target)
                                .await;
                        }
                    }
                    Ok(Err(e)) | Err(e) => {
                        debug!(
                            "Failed to negotiate initial V2 target: at {}, user={} ({:?})",
                            host_and_port, user, e
                        );
                        // TODO consolidate this, so that we have exactly 1 place where we
                        //  initiate failing
                        self.status.initiate_failing();
                    }
                }
            }
            Ok(Err(e)) | Err(e) => {
                error!(
                    "Failed to connect to {}, user={} {:?}",
                    host_and_port, user, e
                );
                self.status.initiate_failing()
            }
        }
    }

    async fn main_task(self: Arc<Self>) {
        // TODO: Count as a discarded solution?
        // Flush all obsolete solutions from previous run
        self.solution_receiver.lock().await.flush();

        loop {
            let mut stop_receiver = self.stop_receiver.lock().await;
            select! {
                _ = self.clone().run().fuse() => {}
                _ = stop_receiver.next() => {}
            }

            // Notify the other end that uses the extension channel that it should restart its
            // operation
            // TODO Note that this error is triggered also when there is not extension channel.
            //  It needs to be reworked once we eliminate the need for a dummy extension channel
            //  pair

            if let Err(e) = self
                .extension_channel_sender
                .lock()
                .await
                .try_send(ExtensionChannelMsg::Stop)
            {
                info!(
                    "Cannot send stop notification into the extension channel: {:?}",
                    e
                );
            }
            // Invalidate current job to stop working on it
            self.job_sender.lock().await.invalidate();
            // Flush all unprocessed solutions to empty buffer
            // TODO: Count as a discarded solution?
            self.solution_receiver.lock().await.flush();
            self.solutions.lock().await.clear();
            // Flush all stop events before stopping!
            while let Ok(Some(_)) = stop_receiver.try_next() {}
            // NOTE: No new stop event cannot be received in this stopping process (another one can
            // be received after calling `can_stop` method where state can be restarted to starting
            // process)

            if self.status.can_stop() {
                // NOTE: it is not safe to add here any code!
                // The reason is that at this point the main task can be executed in parallel again
                break;
            }
            // Restarting
        }
    }

    /// Process a reconnect to a specified host and port. Note, that reconnect doesn't specify
    /// pool authority key. It is not possible (for security reasons) to reconnect to a different
    /// entity that has a different key. The prevents hijacking the hashrate should the upstream
    /// server be compromised.
    /// `host` where to reconnect to (
    async fn reconnect(&self, new_host: String, new_port: u16) {
        let mut connection_details = self
            .connection_details
            .lock()
            .expect("BUG: cannot lock connection details");

        // Change current connection descriptor
        if new_host.len() > 0 {
            // Detect change in host
            if new_host != connection_details.host {
                connection_details.host = new_host;
            }
            // Detect change in port
            if new_port != 0 && connection_details.port != new_port {
                connection_details.port = new_port;
            }
        }
        drop(connection_details);
        // Ignore the result
        self.status.initiate_stopping(|| self.stop());
    }
}

#[async_trait]
impl node::Client for StratumClient {
    fn start(self: Arc<Self>) {
        tokio::spawn(self.clone().main_task());
    }

    fn stop(&self) {
        if let Err(e) = self.stop_sender.clone().try_send(()) {
            assert!(
                e.is_full(),
                "BUG: Unexpected error in stop sender: {}",
                e.to_string()
            );
        }
    }

    async fn get_last_job(&self) -> Option<Arc<dyn job::Bitcoin>> {
        self.last_job
            .lock()
            .await
            .as_ref()
            .map(|job| job.clone() as Arc<dyn job::Bitcoin>)
    }

    /// Build new connection details from the specified `descriptor`
    fn change_connection_details(&self, descriptor: &bosminer_config::ClientDescriptor) {
        *self
            .connection_details
            .lock()
            .expect("BUG: cannot lock connection details") =
            ConnectionDetails::from_descriptor(descriptor);
    }
}

impl fmt::Display for StratumClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let connection_details = self.connection_details();
        write!(
            f,
            "{}://{}@{}",
            connection_details.protocol, connection_details.host, connection_details.user
        )
    }
}

/// TODO: temporary custom Debug implementation that omits most of the fields. The reason is that
/// the derived implementation cannot cover the DynConnectFn
impl fmt::Debug for StratumClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let connection_details = self.connection_details();
        f.debug_struct("StratumClient")
            .field("connection_details", &connection_details)
            .finish()
    }
}
