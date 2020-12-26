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

import logging

import builder.hwid as hwid

from builder.config import EmptyValue

MINER_FIRMWARE = 'firmware'
MINER_ENV_SIZE = 0x20000
MINER_CFG_SIZE = 0x20000

# variables for miner NAND configuration
NET_MAC = 'ethaddr'
NET_IP = 'net_ip'
NET_MASK = 'net_mask'
NET_GATEWAY = 'net_gateway'
NET_DNS_SERVERS = 'net_dns_servers'
NET_HOSTNAME = 'net_hostname'

HW_FREQ = 'miner_freq'
HW_VOLTAGE = 'miner_voltage'
HW_FIXED_FREQ = 'miner_fixed_freq'
HW_PSU_POWER_LIMIT = 'miner_psu_power_limit'

MINER_HWID = 'miner_hwid'
MINER_POOL_HOST = 'miner_pool_host'
MINER_POOL_PORT = 'miner_pool_port'
MINER_POOL_PATH = 'miner_pool_path'
MINER_POOL_USER = 'miner_pool_user'
MINER_POOL_PASS = 'miner_pool_pass'

MINER_CFG_INPUT = [
    (NET_MAC, 'net.mac', None),
    (NET_IP, 'net.ip', ''),
    (NET_MASK, 'net.mask', ''),
    (NET_GATEWAY, 'net.gateway', ''),
    (NET_DNS_SERVERS, 'net.dns_servers', []),
    (NET_HOSTNAME, 'net.hostname', ''),
    (HW_FREQ, 'miner.hw.freq', ''),
    (HW_VOLTAGE, 'miner.hw.voltage', ''),
    (HW_FIXED_FREQ, 'miner.hw.fixed_freq', ''),
    (HW_PSU_POWER_LIMIT, 'miner.hw.psu_power_limit', ''),
    (MINER_HWID, 'miner.hwid', hwid.generate),
    (MINER_POOL_HOST, 'miner.pool.host', ''),
    (MINER_POOL_PORT, 'miner.pool.port', ''),
    (MINER_POOL_PATH, 'miner.pool.path', ''),
    (MINER_POOL_USER, 'miner.pool.user', ''),
    (MINER_POOL_PASS, 'miner.pool.pass', '')
]


def write_miner_cfg_input(config, stream, excluded=None, use_default=True, ignore_empty=True):
    """
    Write to the stream miner configuration input for NAND

    :config:
        Configuration for bOS NAND environment.
    :stream:
        Opened stream for writing miner configuration input.
    :excluded:
        Dictionary with excluded attributes.
    :use_default:
        Use default value for omitted attributes.
    :ignore_empty:
        Not include empty attributes to output for removing it from NAND.
    """
    excluded = excluded or set()
    for name, path, default in MINER_CFG_INPUT:
        if name in excluded:
            continue
        value = config.get(path)
        if value is None and use_default:
            if default is None:
                logging.error("Missing miner configuration for '{}' in '{}'".format(name, path))
                return False
            # use default value when configuration is not set in YAML
            value = default if not callable(default) else default()
        if value:
            # attributes with empty value are completely omitted
            if type(value) not in [str, int]:
                if type(value) is bool:
                    value = str(value).lower()
                elif value.is_list():
                    value = ','.join(value)
            stream.write('{}={}\n'.format(name, value).encode())
        elif value is EmptyValue and not ignore_empty:
            stream.write('{}\n'.format(name).encode())
    return True
