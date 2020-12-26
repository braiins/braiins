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

//! Utilities to calculate baudrates, register packing

use crate::error::{self, ErrorKind};
use packed_struct::prelude::*;

/// Helper method that calculates baud rate clock divisor value for the specified baud rate.
///
/// The calculation follows the same scheme for the hashing chips as well as for the FPGA IP core
///
/// * `baud_rate` - requested baud rate
/// * `base_clock_hz` - base clock for the UART peripheral
/// * `base_clock_div` - divisor for the base clock
/// Return a baudrate divisor and actual baud rate or an error
pub fn calc_baud_clock_div(
    baud_rate: usize,
    base_clock_hz: usize,
    base_clock_div: usize,
) -> error::Result<(usize, usize)> {
    const MAX_BAUD_RATE_ERR_PERC: usize = 5;
    // The actual calculation is:
    // base_clock_hz / (base_clock_div * baud_rate) - 1
    // We have to mathematically round the calculated divisor in fixed point arithmethic
    let baud_div = (10 * base_clock_hz / (base_clock_div * baud_rate) + 5) / 10 - 1;
    let actual_baud_rate = base_clock_hz / (base_clock_div * (baud_div + 1));

    //
    let baud_rate_diff = if actual_baud_rate > baud_rate {
        actual_baud_rate - baud_rate
    } else {
        baud_rate - actual_baud_rate
    };
    // the baud rate has to be within a few percents
    if baud_rate_diff > (MAX_BAUD_RATE_ERR_PERC * baud_rate / 100) {
        Err(ErrorKind::BaudRate(format!(
            "requested {} baud, resulting {} baud",
            baud_rate, actual_baud_rate
        )))?
    }
    Ok((baud_div, actual_baud_rate))
}

/// Just an util trait so that we can pack/unpack directly to registers
pub trait PackedRegister: Sized {
    fn from_reg(reg: u32) -> Self;
    fn to_reg(&self) -> u32;
}

impl<T> PackedRegister for T
where
    T: PackedStruct<[u8; 4]>,
{
    /// Take register and unpack (as big endian)
    fn from_reg(reg: u32) -> Self {
        Self::unpack(&u32::to_be_bytes(reg)).expect("unpacking error")
    }
    /// Pack into big-endian register
    fn to_reg(&self) -> u32 {
        u32::from_be_bytes(self.pack())
    }
}

// compute distance between two usizes
pub fn distance(x: usize, y: usize) -> usize {
    if x >= y {
        x - y
    } else {
        y - x
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use crate::bm1387;
    use crate::io;

    const CHIP_OSC_CLK_HZ: usize = 25_000_000;

    #[test]
    fn test_calc_baud_div_correct_baud_rate_bm1387() {
        // these are sample baud rates for communicating with BM1387 chips
        let correct_bauds_and_divs = [
            (115_200usize, 26usize),
            (460_800, 6),
            (1_500_000, 1),
            (3_000_000, 0),
        ];
        for (baud_rate, baud_div) in correct_bauds_and_divs.iter() {
            let (baud_clock_div, actual_baud_rate) = calc_baud_clock_div(
                *baud_rate,
                CHIP_OSC_CLK_HZ,
                bm1387::CHIP_OSC_CLK_BASE_BAUD_DIV,
            )
            .unwrap();
            assert_eq!(
                baud_clock_div, *baud_div,
                "Calculated baud divisor doesn't match, requested: {} baud, actual: {} baud",
                baud_rate, actual_baud_rate
            )
        }
    }

    #[test]
    fn test_calc_baud_div_correct_baud_rate_fpga() {
        // these are baudrates commonly used with UART on FPGA
        let correct_bauds_and_divs = [(115_740usize, 53usize), (1_562_500, 3), (3_125_000, 1)];
        for &(baud_rate, baud_div) in correct_bauds_and_divs.iter() {
            let (baud_clock_div, _actual_baud_rate) =
                calc_baud_clock_div(baud_rate, io::F_CLK_SPEED_HZ, io::F_CLK_BASE_BAUD_DIV)
                    .expect("failed to calculate divisor");
            assert_eq!(baud_clock_div, baud_div);
        }
    }

    /// Test higher baud rate than supported
    #[test]
    fn test_calc_baud_div_over_baud_rate_bm1387() {
        let result = calc_baud_clock_div(
            3_500_000,
            CHIP_OSC_CLK_HZ,
            bm1387::CHIP_OSC_CLK_BASE_BAUD_DIV,
        );
        assert!(
            result.is_err(),
            "Baud clock divisor unexpectedly calculated!"
        );
    }
}
