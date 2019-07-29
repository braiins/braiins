use super::*;
use crate::btc;
use crate::hal;
use crate::work;

use crate::misc::LOGGER;
use slog::{info, trace};

use futures::channel::mpsc;
use futures::stream::StreamExt;

use std::sync::{Arc, RwLock};

/// Top level builder for `JobSolver` and `work::Solver` intended to be used when instantiating
/// the full miner
pub struct Hub;

impl Hub {
    /// Create Solvers for frontend (pool) and backend (HW accelerator)
    pub fn build_solvers() -> (JobSolver, work::Solver) {
        let (engine_sender, engine_receiver) = engine_channel();
        let (solution_queue_tx, solution_queue_rx) = mpsc::unbounded();
        (
            JobSolver::new(engine_sender, solution_queue_rx),
            work::Solver::new(engine_receiver, solution_queue_tx),
        )
    }
}

/// Helper function for creating target difficulty suitable for sharing
pub fn create_shared_target(value: uint::U256) -> Arc<RwLock<uint::U256>> {
    Arc::new(RwLock::new(value))
}

/// Compound object for job submission and solution reception intended to be passed to
/// protocol handler
pub struct JobSolver {
    job_sender: JobSender,
    solution_receiver: JobSolutionReceiver,
}

impl JobSolver {
    pub fn new(
        engine_sender: EngineSender,
        solution_queue_rx: mpsc::UnboundedReceiver<hal::UniqueMiningWorkSolution>,
    ) -> Self {
        let current_target =
            create_shared_target(uint::U256::from_big_endian(&btc::DIFFICULTY_1_TARGET_BYTES));
        Self {
            job_sender: hub::JobSender::new(engine_sender, current_target.clone()),
            solution_receiver: hub::JobSolutionReceiver::new(solution_queue_rx, current_target),
        }
    }

    pub fn split(self) -> (JobSender, JobSolutionReceiver) {
        (self.job_sender, self.solution_receiver)
    }
}

/// This is the entrypoint for new jobs and updates into processing.
/// Typically the mining protocol handler will inject new jobs through it
pub struct JobSender {
    engine_sender: EngineSender,
    current_target: Arc<RwLock<uint::U256>>,
}

impl JobSender {
    pub fn new(engine_sender: EngineSender, current_target: Arc<RwLock<uint::U256>>) -> Self {
        Self {
            engine_sender,
            current_target,
        }
    }

    pub fn change_target(&self, target: uint::U256) {
        *self
            .current_target
            .write()
            .expect("cannot write to shared current target") = target;
    }

    pub fn send(&mut self, job: Arc<dyn hal::BitcoinJob>) {
        info!(LOGGER, "--- broadcasting new job ---");
        let engine = Arc::new(engine::VersionRolling::new(job, 1));
        self.engine_sender.broadcast(engine);
    }
}

/// Receives `UniqueMiningWorkSolution` via a channel and filters only solutions that meet the
/// pool specified target
pub struct JobSolutionReceiver {
    solution_channel: mpsc::UnboundedReceiver<hal::UniqueMiningWorkSolution>,
    current_target: Arc<RwLock<uint::U256>>,
}

impl JobSolutionReceiver {
    pub fn new(
        solution_channel: mpsc::UnboundedReceiver<hal::UniqueMiningWorkSolution>,
        current_target: Arc<RwLock<uint::U256>>,
    ) -> Self {
        Self {
            solution_channel,
            current_target,
        }
    }

    fn trace_share(solution: &hal::UniqueMiningWorkSolution, target: &uint::U256) {
        // TODO: create specialized structure 'Target' and rewrite it
        let mut xtarget = [0u8; 32];
        target.to_big_endian(&mut xtarget);

        trace!(
            LOGGER,
            "nonce={:08x} bytes={}",
            solution.nonce(),
            hex::encode(&solution.get_block_header().into_bytes()[..])
        );
        trace!(LOGGER, "  hash={:x}", solution.hash());
        trace!(LOGGER, "target={}", hex::encode(xtarget));
    }

    pub async fn receive(&mut self) -> Option<hal::UniqueMiningWorkSolution> {
        while let Some(solution) = await!(self.solution_channel.next()) {
            let current_target = &*self
                .current_target
                .read()
                .expect("cannot read from shared current target");
            if solution.is_valid(current_target) {
                info!(LOGGER, "----- SHARE BELLOW TARGET -----");
                Self::trace_share(&solution, &current_target);
                return Some(solution);
            }
        }
        None
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::test_utils;
    use crate::utils::compat_block_on;

    use bitcoin_hashes::{sha256d::Hash, Hash as HashTrait};

    #[test]
    fn test_solvers_connection() {
        let (job_solver, work_solver) = work::Hub::build_solvers();

        let (mut job_sender, mut solution_receiver) = job_solver.split();
        let (mut work_generator, solution_sender) = work_solver.split();

        // default target is be set to difficulty 1 so all solution should pass
        for block in test_utils::TEST_BLOCKS.iter() {
            let job = Arc::new(*block);

            // send prepared testing block to job solver
            job_sender.send(job);
            // work generator receives this job and prepares work from it
            let work = compat_block_on(work_generator.generate()).unwrap();
            // initial value for version rolling is 0 so midstate should match with expected one
            assert_eq!(block.midstate, work.midstates[0].state);
            // test block has automatic conversion into work solution
            solution_sender.send(block.into());
            // this solution should pass through job solver
            let solution = compat_block_on(solution_receiver.receive()).unwrap();
            // check if the solution is equal to expected one
            assert_eq!(block.nonce, solution.nonce());
            let original_job: &test_utils::TestBlock = solution.job();
            // the job should also match with original one
            // job solver does not returns Arc so the comparison is done by its hashes
            assert_eq!(block.hash, original_job.hash);
        }

        // work generator still works even if all job solvers are dropped
        drop(job_sender);
        drop(solution_receiver);
        assert!(compat_block_on(work_generator.generate()).is_some());
    }

    fn double_hash_cmp(a: &Hash, b: &Hash) -> std::cmp::Ordering {
        let a_u256 = uint::U256::from_little_endian(&a.into_inner());
        let b_u256 = uint::U256::from_little_endian(&b.into_inner());
        a_u256.cmp(&b_u256)
    }

    #[test]
    fn test_job_solver_target() {
        let (engine_sender, _) = engine_channel();
        let (solution_queue_tx, solution_queue_rx) = mpsc::unbounded();

        let (job_sender, mut solution_receiver) =
            JobSolver::new(engine_sender, solution_queue_rx).split();

        // default target is be set to difficulty 1 so all solution should pass
        for block in test_utils::TEST_BLOCKS.iter() {
            // test block has automatic conversion into work solution
            solution_queue_tx.unbounded_send(block.into()).unwrap();
            // this solution should pass through job solver
            let solution = compat_block_on(solution_receiver.receive()).unwrap();
            // check if the solution is equal to expected one
            assert_eq!(block.nonce, solution.nonce());
        }

        // find test block with lowest hash which will be set as a target
        let target_block = test_utils::TEST_BLOCKS
            .iter()
            .min_by(|a, b| double_hash_cmp(&a.hash, &b.hash))
            .unwrap();

        // change the target to return from solution receiver only this block
        let target = uint::U256::from_little_endian(&target_block.hash.into_inner());
        job_sender.change_target(target);

        // send all solutions to the queue not to block on receiver
        for block in test_utils::TEST_BLOCKS.iter() {
            // test block has automatic conversion into work solution
            solution_queue_tx.unbounded_send(block.into()).unwrap();
        }

        // send target block again to get two results and ensures that all blocks has been processed
        solution_queue_tx
            .unbounded_send(target_block.into())
            .unwrap();

        // check if the solutions is equal to expected ones
        let solution = compat_block_on(solution_receiver.receive()).unwrap();
        assert_eq!(target_block.nonce, solution.nonce());
        let solution = compat_block_on(solution_receiver.receive()).unwrap();
        assert_eq!(target_block.nonce, solution.nonce());
    }
}