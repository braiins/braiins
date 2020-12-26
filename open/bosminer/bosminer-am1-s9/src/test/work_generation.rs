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

use ii_logging::macros::*;

use super::*;
use crate::bm1387::MidstateCount;
use crate::fan;
use crate::hashchain;
use crate::registry;

use bosminer::work;

use std::time::Duration;

use std::sync::Arc;

use futures::channel::mpsc;
use futures::stream::StreamExt;

use ii_async_compat::{tokio, FutureExt};
use tokio::time::delay_for;

const ASIC_DIFFICULTY: usize = 1;

/// Prepares sample work with empty midstates
/// NOTE: this work has 2 valid nonces:
/// - 0x83ea0372 (solution 0)
/// - 0x09f86be1 (solution 1)
fn prepare_test_work(midstate_count: usize) -> work::Assignment {
    let time = 0xffffffff;
    let job = Arc::new(null_work::NullJob::new(time, 0xffff_ffff, 0));

    let one_midstate = work::Midstate {
        version: 0,
        state: [0u8; 32].into(),
    };
    work::Assignment::new(job, vec![one_midstate; midstate_count], time)
}

/// Task that receives solutions from hardware and sends them to channel
async fn receiver_task(
    hash_chain: Arc<hashchain::HashChain>,
    solution_sender: mpsc::UnboundedSender<hashchain::Solution>,
) {
    let mut rx_io = hash_chain.take_work_rx_io().await;
    let target = ii_bitcoin::Target::from_pool_difficulty(ASIC_DIFFICULTY);

    loop {
        let (rx_io_out, solution) = rx_io.recv_solution().await.expect("recv solution");
        rx_io = rx_io_out;
        solution_sender
            .unbounded_send(hashchain::Solution::from_hw_solution(&solution, target))
            .expect("solution send failed");
    }
}

/// Task that receives work from channel and sends it to HW
async fn sender_task(
    hash_chain: Arc<hashchain::HashChain>,
    mut work_receiver: mpsc::UnboundedReceiver<work::Assignment>,
) {
    let mut tx_io = hash_chain.take_work_tx_io().await;
    let mut work_registry =
        registry::WorkRegistry::<hashchain::Solution>::new(tx_io.work_id_count());

    loop {
        tx_io.wait_for_room().await.expect("wait for tx room");
        let work = work_receiver.next().await.expect("failed receiving work");
        let work_id = work_registry.store_work(work.clone(), false);
        // send work is synchronous
        tx_io.send_work(&work, work_id).expect("send work");
    }
}

async fn send_and_receive_test_workloads<'a>(
    work_sender: &'a mpsc::UnboundedSender<work::Assignment>,
    solution_receiver: &'a mut mpsc::UnboundedReceiver<hashchain::Solution>,
    n_send: usize,
    expected_solution_count: usize,
) {
    info!(
        "Sending {} work items and trying to receive {} solutions",
        n_send, expected_solution_count,
    );
    //
    // Put in some tasks
    for _ in 0..n_send {
        let work = prepare_test_work(1);
        work_sender.unbounded_send(work).expect("work send failed");
        // wait time to send out work + to compute work
        // TODO: come up with a formula instead of fixed time interval
        // wait = work_time * number_of_chips + time_to_send_out_a_jov

        delay_for(Duration::from_millis(100)).await;
    }
    let mut returned_solution_count = 0;
    while let Ok(res) = solution_receiver
        .next()
        .timeout(Duration::from_millis(1000))
        .await
    {
        res.expect("timeout error");
        returned_solution_count += 1;
    }

    assert_eq!(
        returned_solution_count, expected_solution_count,
        "expected {} solutions but got {}",
        expected_solution_count, returned_solution_count
    );
}

async fn start_hchain(monitor_tx: mpsc::UnboundedSender<monitor::Message>) -> hashchain::HashChain {
    let hashboard_idx = config::S9_HASHBOARD_INDEX;
    let gpio_mgr = gpio::ControlPinManager::new();
    let voltage_ctrl_backend = Arc::new(power::I2cBackend::new(0));
    let fan_control = fan::Control::new().expect("failed initializing fan controller");
    let reset_pin =
        hashchain::ResetPin::open(&gpio_mgr, hashboard_idx).expect("failed to make pin");
    let plug_pin = hashchain::PlugPin::open(&gpio_mgr, hashboard_idx).expect("failed to make pin");

    // turn on fans to full (no temp control)
    fan_control.set_speed(fan::Speed::FULL_SPEED);

    let mut hash_chain = hashchain::HashChain::new(
        reset_pin,
        plug_pin,
        voltage_ctrl_backend.clone(),
        hashboard_idx,
        MidstateCount::new(1),
        ASIC_DIFFICULTY,
        monitor_tx,
    )
    .unwrap();
    hash_chain.disable_init_work = true;

    hash_chain
        .init(
            &hashchain::FrequencySettings::from_frequency(
                (config::DEFAULT_FREQUENCY_MHZ * 1_000_000.0) as usize,
            ),
            *crate::power::OPEN_CORE_VOLTAGE,
            true,
        )
        .await
        .expect("h_chain init failed");
    hash_chain
}

/// Verifies work generation for a hash chain
///
/// The test runs two batches of work:
/// - the first 3 work items are for initializing input queues of the chips and don't provide any
/// solutions
/// - the next 2 work items yield actual solutions. Since we don't push more work items, the
/// solution 1 never appears on the bus and leave chips output queues. This is fine as this test
/// is intended for initial check of correct operation
#[tokio::test]
async fn test_work_generation() {
    // Create channels
    let (solution_sender, mut solution_receiver) = mpsc::unbounded();
    let (work_sender, work_receiver) = mpsc::unbounded();
    let (monitor_sender, _monitor_receiver) = mpsc::unbounded();

    // Guard lives until the end of the block
    let _work_sender_guard = work_sender.clone();
    let _solution_sender_guard = solution_sender.clone();

    // Start HW
    let hash_chain = Arc::new(start_hchain(monitor_sender).await);

    // start HW receiver
    tokio::spawn(receiver_task(hash_chain.clone(), solution_sender));

    // start HW sender
    tokio::spawn(sender_task(hash_chain.clone(), work_receiver));

    // the first 3 work loads don't produce any solutions, these are merely to initialize the input
    // queue of each hashing chip
    send_and_receive_test_workloads(&work_sender, &mut solution_receiver, 3, 0).await;

    // submit 2 more work items, since we are intentionally being slow all chips should send a
    // solution for the submitted work
    let more_work_count = 2usize;
    let chip_count = hash_chain.get_chip_count();
    let expected_solution_count = more_work_count * chip_count;

    send_and_receive_test_workloads(
        &work_sender,
        &mut solution_receiver,
        more_work_count,
        expected_solution_count,
    )
    .await;

    // stop everything
    hash_chain.halt_sender.clone().send_halt().await;
}
