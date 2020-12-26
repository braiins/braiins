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

use ii_async_compat::tokio;
use tokio::time::delay_for;

use bosminer_am1_s9::gpio;
use bosminer_am1_s9::power;
use bosminer_am1_s9::{hashchain::ResetPin, Backend};

use std::sync::Arc;

use std::time::Duration;

/// Helper function that tests voltage controller on a particular hashboard.
///
/// The test simply verifies that the voltage controller responds with a valid version
///
/// * `hashboard_idx` - index of the hashboard
/// * `gpio_mgr` - provides accesss to  GPIO control pins connected to the hashboard
async fn test_voltage_ctrl_on_1_hashboard(
    gpio_mgr: &gpio::ControlPinManager,
    hashboard_idx: usize,
) {
    let mut reset_pin =
        ResetPin::open(gpio_mgr, hashboard_idx).expect("BUG: failed to make reset pin");

    // perform reset of the hashboard
    reset_pin.enter_reset().unwrap();
    delay_for(Duration::from_secs(1)).await;
    reset_pin.exit_reset().unwrap();

    let backend = Arc::new(power::I2cBackend::new(0));
    let voltage_ctrl = power::Control::new(backend, hashboard_idx);

    voltage_ctrl.reset().await.unwrap();
    voltage_ctrl.jump_from_loader_to_app().await.unwrap();

    let version = voltage_ctrl.get_version().await.unwrap();
    let expected_version: u8 = 3;
    assert_eq!(
        version, expected_version,
        "Expected version {:x}",
        expected_version
    );
}

/// Attempts to run voltage controller test for all hashboards. A minimum of one hashboard is
/// required to be present in the miner
#[tokio::test]
async fn test_voltage_ctrl_all_hashboards() {
    let mut tested_hashboards: usize = 0;
    let expected_tested_hashboards: usize = 1;

    let gpio_mgr = gpio::ControlPinManager::new();
    for hashboard_idx in
        Backend::detect_hashboards(&gpio_mgr).expect("BUG: failed to detect hashboards")
    {
        test_voltage_ctrl_on_1_hashboard(&gpio_mgr, hashboard_idx).await;
        tested_hashboards += 1;
    }
    assert!(
        tested_hashboards >= expected_tested_hashboards,
        "Not enough hashboards found, cannot test voltage controller, tested: {} expected:{}",
        tested_hashboards,
        expected_tested_hashboards
    );
}
