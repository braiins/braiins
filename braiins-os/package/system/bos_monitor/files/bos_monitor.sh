#!/bin/sh

# Copyright (C) 2020  Braiins Systems s.r.o.
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

# opkg list-upgradable | awk '/firmware/ {print $3 " -> " $5}' > /tmp/bos_upgrade

if [ $# -eq 0 ]; then
	# run i-notify daemon
	exec inotifyd "$0" /var/lock/:d
fi

# user space agent
# $1. actual event(s)
# $2. file (or directory) name
# $3. name of subfile (if any), in case of watching a directory

OPKG_CONF_PATH="/etc/opkg.conf"

BOS_FIRMWARE_NAME="bos_firmware"
BOS_UPGRADE_PATH="/tmp/bos_upgrade"

has_new_feeds() {
	local feed_path
	for feed_path in "$@"; do
		# skip all files with sig extension
		case "$feed_path" in
		*.sig)
			continue
			;;
		esac
		[ -f "$feed_path" -a "(" ! -f "$BOS_UPGRADE_PATH" -o "$BOS_UPGRADE_PATH" -ot "$feed_path" ")" ] \
			&& return 0
	done
	# everything is up to date
	return 1
}

case "$3" in
opkg.lock)
	opkg_lists=$(awk '/lists_dir/ {print $3}' /etc/opkg.conf)
	if has_new_feeds "${opkg_lists}/${BOS_FIRMWARE_NAME}"*; then
		touch "$BOS_UPGRADE_PATH"
		bos_upgradable=$(opkg list-upgradable | awk '/firmware/ {print $3 " -> " $5}')
		echo "$bos_upgradable" > "$BOS_UPGRADE_PATH"
	fi
	;;
*)
	;;
esac
