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

use crate::MidstateCount;

/// This structure represents address of a single work midstate in FPGA core.
/// The address is compound of `work_id` and `midstate_idx`.
///
/// In FPGA core, this value is encoded as 16-bit word with some number
/// of bits allocated to `midstate_idx` and some to `work_id`, depending
/// on the midstate count configuration (ie. if IP is configured for 4
/// midstates, then 2 bits are allocated for `midstate_idx` and 14 for
/// `work_id`).
///
/// **Note**: this representation is specific to FPGA IP core we use.
/// The hardware chip itself uses a different `work_id`: the chip `work_id`
/// is just 7-bit (for midstate and `work_id`) and the FPGA IP core then
/// extends this number to 16-bit by keeping track of what `work_id` was
/// sent to hardware last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtWorkId {
    pub work_id: usize,
    /// Midstate index is relevant only when receiving a work solution denoting the midstate index
    /// relevant for a specific solution
    pub midstate_idx: usize,
}

impl ExtWorkId {
    /// Range is 16 bits
    const EXT_WORK_ID_COUNT: u32 = 0x10000;

    pub fn new(work_id: usize, midstate_idx: usize) -> Self {
        Self {
            work_id,
            midstate_idx,
        }
    }

    /// Compute the range of `work_id` that can be stored in (16 bit)
    /// `ext_work_id`. For example if two bits are used for midstates,
    /// we can use only 14 bits for `work_id` so return the number of
    /// work_ids that can fit in that.
    pub fn get_work_id_count(midstate_count: MidstateCount) -> usize {
        (Self::EXT_WORK_ID_COUNT >> midstate_count.to_bits()) as usize
    }

    /// Create new `ExtWorkId` from FPGA core representation: divide
    /// the word into `midstate_idx` and `work_id` parts depending
    /// on the number of midstates we are using.
    /// As ext_id should be 16 bit, check if it isn't too large.
    pub fn from_hw(midstate_count: MidstateCount, ext_id: u32) -> Self {
        assert!(ext_id < Self::EXT_WORK_ID_COUNT);
        let ext_id = ext_id as usize;
        Self {
            work_id: ext_id >> midstate_count.to_bits(),
            midstate_idx: ext_id & midstate_count.to_mask(),
        }
    }

    /// Serialize `Self` to FPGA core representation.
    pub fn to_hw(&self, midstate_count: MidstateCount) -> u32 {
        assert!(self.work_id < Self::get_work_id_count(midstate_count));
        assert!(self.midstate_idx < midstate_count.to_count());

        ((self.work_id << midstate_count.to_bits()) | self.midstate_idx) as u32
    }
}

#[cfg(test)]
pub mod test_utils {
    use super::*;

    /// Test that `ExtWorkId` gets deserialized correctly
    #[test]
    fn test_from_hw() {
        assert_eq!(
            ExtWorkId::from_hw(MidstateCount::new(1), 0x8765),
            ExtWorkId::new(0x8765, 0)
        );
        assert_eq!(
            ExtWorkId::from_hw(MidstateCount::new(2), 0x8765),
            ExtWorkId::new(0x43b2, 1)
        );
        assert_eq!(
            ExtWorkId::from_hw(MidstateCount::new(4), 0x8765),
            ExtWorkId::new(0x21d9, 1)
        );
    }

    /// Test that `ExtWorkId` gets serialized correctly
    #[test]
    fn test_to_hw() {
        assert_eq!(
            ExtWorkId::new(0x8765, 0).to_hw(MidstateCount::new(1)),
            0x8765
        );
        assert_eq!(
            ExtWorkId::new(0x43b2, 1).to_hw(MidstateCount::new(2)),
            0x8765
        );
        assert_eq!(
            ExtWorkId::new(0x21d9, 1).to_hw(MidstateCount::new(4)),
            0x8765
        );
    }

    /// Test that trying to serialize `ExtWorkId` that would overflow the 16bit field will panic
    #[test]
    #[should_panic]
    fn test_to_hw_fail() {
        ExtWorkId::new(0x8765, 2).to_hw(MidstateCount::new(2));
    }

    /// Test that trying to serialize `ExtWorkId` with too high `midstate_idx` would fail
    #[test]
    #[should_panic]
    fn test_to_hw_fail_2() {
        ExtWorkId::new(0x8765, 1).to_hw(MidstateCount::new(1));
    }

    #[test]
    fn test_work_id_count() {
        assert_eq!(
            ExtWorkId::get_work_id_count(MidstateCount::new(1)),
            0x10_000
        );
        assert_eq!(ExtWorkId::get_work_id_count(MidstateCount::new(2)), 0x8_000);
        assert_eq!(ExtWorkId::get_work_id_count(MidstateCount::new(4)), 0x4_000);
    }
}
