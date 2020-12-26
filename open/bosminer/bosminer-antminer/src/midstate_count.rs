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

/// `MidstateCount` represents the number of midstates FPGA sends to chips.
/// This information needs to be accessible to everyone that processes `work_id`.
///
/// `MidstateCount` provides methods to encode number of midstates in various ways:
///  * bitmask to mask out parts of `solution_id`
///  * base-2 logarithm of number of midstates
///
/// `MidstateCount` is always valid - creation of `MidstateCount` object that isn't
/// supported by hardware shouldn't be possible.
#[derive(Debug, Clone, Copy)]
pub struct MidstateCount {
    /// internal representation is base-2 logarithm of number of midstates
    log2: usize,
}

impl MidstateCount {
    /// Construct Self, panic if number of midstates is not valid for this hw
    pub fn new(count: usize) -> Self {
        match count {
            1 => Self { log2: 0 },
            2 => Self { log2: 1 },
            4 => Self { log2: 2 },
            _ => panic!("Unsupported midstate count {}", count),
        }
    }

    /// Return midstate count
    #[inline]
    pub fn to_count(&self) -> usize {
        1 << self.log2
    }

    /// Return log2 of midstate count
    #[inline]
    pub fn to_bits(&self) -> usize {
        self.log2
    }

    /// Return midstate count mask (to get midstate_idx bits from `work_id`)
    #[inline]
    pub fn to_mask(&self) -> usize {
        (1 << self.log2) - 1
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_midstate_count_instance() {
        MidstateCount::new(1);
        MidstateCount::new(2);
        MidstateCount::new(4);
    }

    #[test]
    #[should_panic]
    fn test_midstate_count_instance_fail() {
        MidstateCount::new(3);
    }

    #[test]
    fn test_midstate_count_conversion() {
        assert_eq!(MidstateCount::new(4).to_mask(), 3);
        assert_eq!(MidstateCount::new(2).to_count(), 2);
    }
}
