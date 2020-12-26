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

set -e

MONOREPO_DIR=/src
BOS_DIR=braiins-os
RELEASE_BUILD_DIR=$MONOREPO_DIR/$BOS_DIR

DOCKER_SSH_AUTH_SOCK=/ssh-agent
USER_NAME=build
HOST_ROOT_DIR=$PWD/..

if [ $# -eq 0 ]; then
    echo "Warning: Missing build release parameters!"
    echo "Running only braiins OS build environment..."
else
    ARGS="./build.sh $@"
fi

docker run -it --rm \
    -v $HOME/.ssh/known_hosts:/home/$USER_NAME/.ssh/known_hosts:ro \
    -v $SSH_AUTH_SOCK:$DOCKER_SSH_AUTH_SOCK -e SSH_AUTH_SOCK=$DOCKER_SSH_AUTH_SOCK \
    -v $HOST_ROOT_DIR:$MONOREPO_DIR -w $RELEASE_BUILD_DIR \
    -e RELEASE_BUILD_DIR=$RELEASE_BUILD_DIR \
    $USER/bos-builder $ARGS
