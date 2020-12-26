#!/bin/sh -e

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

# update system with missing utilities
cp system/ld-musl-armhf.so.1 /lib
chmod +x /lib/ld-musl-armhf.so.1

cp system/fw_printenv /usr/sbin
chmod +x /usr/sbin/fw_printenv

ln -fs /usr/sbin/fw_printenv /usr/sbin/fw_setenv

ETHADDR=$(cat /sys/class/net/eth0/address)

# create hardware id
echo $ETHADDR >/dev/urandom
MINER_HWID=$(dd if=/dev/urandom bs=1 count=12 2>/dev/null | base64 | tr +/ ab)

# change current directory to firmware
cd firmware

# run stage 1 upgrade process
if ! /bin/sh stage1.sh "$MINER_HWID" "" default yes cond yes no >/dev/null; then
	# clean up system to left it untouched
	rm /usr/sbin/fw_setenv
	rm /usr/sbin/fw_printenv
	rm /lib/ld-musl-armhf.so.1
	exit 1
fi
