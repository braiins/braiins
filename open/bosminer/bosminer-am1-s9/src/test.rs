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

pub mod work_generation;

use bm1387::MidstateCount;

use super::*;

#[tokio::test]
async fn test_hchain_ctl_instance() {
    let hashboard_idx = config::S9_HASHBOARD_INDEX;
    let gpio_mgr = gpio::ControlPinManager::new();
    let voltage_ctrl_backend = Arc::new(power::I2cBackend::new(0));
    let (monitor_sender, _monitor_receiver) = mpsc::unbounded();
    let reset_pin =
        hashchain::ResetPin::open(&gpio_mgr, hashboard_idx).expect("BUG: failed to make pin");
    let plug_pin =
        hashchain::PlugPin::open(&gpio_mgr, hashboard_idx).expect("BUG: failed to make pin");

    let hash_chain = hashchain::HashChain::new(
        reset_pin,
        plug_pin,
        voltage_ctrl_backend,
        hashboard_idx,
        MidstateCount::new(1),
        config::DEFAULT_ASIC_DIFFICULTY,
        monitor_sender,
    );
    match hash_chain {
        Ok(_) => assert!(true),
        Err(e) => assert!(false, "Failed to instantiate hash chain, error: {}", e),
    }
}
