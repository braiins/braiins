// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU Common Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Common Public License for more details.
//
// You should have received a copy of the GNU Common Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

//! Extension of PID controller that adds configurable offset so that control variable of "0" could
//! have a different interpretation.

use pid_control::{Controller, PIDController};

pub struct OffsetPIDController {
    pid: PIDController,
    offset: f64,
}

impl OffsetPIDController {
    pub fn new(p_gain: f64, i_gain: f64, d_gain: f64, offset: f64) -> Self {
        Self {
            pid: PIDController::new(p_gain, i_gain, d_gain),
            offset,
        }
    }

    pub fn set_limits(&mut self, min: f64, max: f64) {
        self.pid.set_limits(min - self.offset, max - self.offset);
    }
}

impl Controller for OffsetPIDController {
    fn set_target(&mut self, target: f64) {
        self.pid.set_target(target);
    }

    fn target(&self) -> f64 {
        self.pid.target()
    }

    fn update(&mut self, value: f64, delta_t: f64) -> f64 {
        self.pid.update(value, delta_t) + self.offset
    }

    fn reset(&mut self) {
        self.pid.reset()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use approx::assert_relative_eq;

    /// Verify that offset is added to the results of PID controller computation
    #[test]
    fn test_pid_offset() {
        let mut pid = OffsetPIDController::new(0.0, 0.0, 0.0, 50.0);
        assert_relative_eq!(pid.update(0.0, 1.0), 50.0);
        pid.set_limits(60.0, 60.0);
        assert_relative_eq!(pid.update(0.0, 1.0), 60.0);
    }
}
