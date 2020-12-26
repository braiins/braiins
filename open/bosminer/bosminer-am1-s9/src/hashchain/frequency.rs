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

//! Chip frequency settings

use std::fmt;

type Frequency = usize;

#[derive(Clone)]
pub struct FrequencySettings {
    pub chip: Vec<Frequency>,
}

impl FrequencySettings {
    /// Build frequency settings with all chips having the same frequency
    pub fn from_frequency(frequency: usize) -> Self {
        Self {
            chip: vec![frequency; super::EXPECTED_CHIPS_ON_CHAIN],
        }
    }

    pub fn set_chip_count(&mut self, chip_count: usize) {
        assert!(self.chip.len() >= chip_count);
        self.chip.resize(chip_count, 0);
    }

    pub fn total(&self) -> u64 {
        self.chip.iter().fold(0, |total_f, &f| total_f + f as u64)
    }

    #[allow(dead_code)]
    pub fn min(&self) -> usize {
        *self.chip.iter().min().expect("BUG: no chips on chain")
    }

    #[allow(dead_code)]
    pub fn max(&self) -> usize {
        *self.chip.iter().max().expect("BUG: no chips on chain")
    }

    pub fn avg(&self) -> usize {
        assert!(self.chip.len() > 0, "BUG: no chips on chain");
        let sum: u64 = self.chip.iter().map(|frequency| *frequency as u64).sum();
        (sum / self.chip.len() as u64) as usize
    }

    fn pretty_frequency(freq: usize) -> String {
        format!("{:.01} MHz", (freq as f32) / 1_000_000.0)
    }
}

impl fmt::Display for FrequencySettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let min = self.min();
        let max = self.max();
        if min == max {
            write!(f, "{} (all chips)", Self::pretty_frequency(min))
        } else {
            write!(
                f,
                "{} (min {}, max {})",
                Self::pretty_frequency((self.total() / (self.chip.len() as u64)) as Frequency),
                Self::pretty_frequency(min),
                Self::pretty_frequency(max)
            )
        }
    }
}
