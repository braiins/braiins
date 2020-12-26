#!/bin/bash

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

# Purpose: release script for braiins OS firmware

# The script:
# - runs a build of braiins-os for all specified targets
# - and generates scripts for packaging and signing the resulting build of
#
#
# Synopsis: ./build.sh

set -e
TARGET=zynq-am1-s9

version=$(./bb.py build-version)
echo "Building $version..."

./bb.py --platform $TARGET prepare
./bb.py --platform $TARGET clean
./bb.py --platform $TARGET prepare --update-feeds
./bb.py --platform $TARGET build
./bb.py --platform $TARGET deploy

upgrade_file="braiins-os_am1-s9_ssh_${version}.tar.gz"
script_dir=`dirname $0`
${script_dir}/mkimage.sh output/$TARGET/sd output/$TARGET/upgrade/$upgrade_file output/$TARGET/
