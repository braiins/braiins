#!/bin/sh

# Copyright (C) 2019  Braiins Systems s.r.o.
#
# This file is part of Braiins Open-Source Initiative (BOSI).
#
# BOSI is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.
#
# Please, keep in mind that we may also license BOSI or any part thereof
# under a proprietary license. For more information on the terms and conditions
# of such proprietary license or if you have any other questions, please
# contact us at opensource@braiins.com.

set -e

DEVICE_CFG="device_cfg"

mtd_write() {
	mtd -e "$2" write "$1" "$2"
}

echo "Running stage2 upgrade process..."

# dump all network and miner settings
fw_printenv 2> /dev/null \
| grep '^\(ethaddr\)\|\(net_\)\|\(miner_\)' > "$DEVICE_CFG"

# turn off error checking for auxiliary settings
set +e
STAGE3_OFFSET=$(fw_printenv -n stage3_off 2> /dev/null)
STAGE3_SIZE=$(fw_printenv -n stage3_size 2> /dev/null)
STAGE3_MTD=/dev/mtd$(fw_printenv -n stage3_mtd 2> /dev/null)
STAGE3_PATH="/tmp/stage3.tgz"
set -e

mtd_write fit.itb recovery
mtd -n -p 0x0600000 write factory.bin.gz recovery
mtd -n -p 0x1400000 write system.bit.gz recovery
mtd -n -p 0x1500000 write boot.bin.gz recovery
mtd -n -p 0x1520000 write uboot.img.gz recovery

mtd_write miner_cfg.bin miner_cfg
fw_setenv -c miner_cfg.config --script "$DEVICE_CFG"

if [ -n "${STAGE3_SIZE}" ]; then
	# detected stage3 upgrade tarball
	nanddump -s ${STAGE3_OFFSET} -l ${STAGE3_SIZE} -f "${STAGE3_PATH}" ${STAGE3_MTD}
fi

mtd erase uboot_env
mtd erase fpga1
mtd erase fpga2
mtd erase firmware1
mtd erase firmware2

if [ -n "${STAGE3_SIZE}" ]; then
	# write size of stage3 tarball to the first block of firmware2 partition
	printf "0: %.8x" ${STAGE3_SIZE} | xxd -r -g0 | mtd -n write - firmware2
	# write stage3 upgrade tarball to firmware2 partition after header block
	mtd -n -p 0x0100000 write "${STAGE3_PATH}" firmware2
fi

sync
