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

# awk -f jq.awk -f JSON.awk -v STREAM=0 -v _JQ_SELECT="path" file ...
# awk -f jq.awk -f JSON.awk -v STREAM=0 -v _JQ_COUNT="path.to.array" file ...

function cb_parse_array_empty(jpath) {
	_CFG_ARRAY_EMPTY = 1
	_CFG_PREV_JPATH = jpath
	return ""
}

function cb_parse_object_empty(jpath) {
	return ""
}

function cb_parse_array_enter(jpath) {
	return
}

function cb_parse_array_exit(jpath, status) {
	if ("" != _CFG_PREV_JPATH && _JQ_COUNT == jpath) {
		if (_CFG_ARRAY_EMPTY) {
			print 0
		} else {
			print int(substr(_CFG_PREV_JPATH, length(jpath) + 2)) + 1
		}
		STATUS = 0; exit
	}
	_CFG_ARRAY_EMPTY = 0
}

function cb_parse_object_enter(jpath) {
	return
}

function cb_parse_object_exit(jpath, status) {
	return
}

function cb_append_jpath_component(jpath, component) {
	if ("" == component) {
		return ""
	} else {
		if (component ~ /^".*"$/) {
			component = substr(component, 2, length(component) - 2)
		}
		if ("" == jpath) {
			return component
		} else {
			return jpath "." component
		}
	}
}

function cb_append_jpath_value(jpath, value) {
	if (_JQ_SELECT == jpath) {
		if (value ~ /^".*"$/) {
			value = substr(value, 2, length(value) - 2)
		}
		print value
		STATUS = 0; exit
	}
	_CFG_PREV_JPATH = jpath
	return ""
}

function cb_jpaths(jpaths, njpaths) {
	return
}

function cb_fails(fails, nfails) {
	return
}

function cb_fail1(message) {
	print m
	STATUS = 1
	exit
}

END {
	exit(STATUS)
}
