#!/usr/bin/env python3

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

import sys
import argparse
import logging
import colorlog
import builder as bos_builder
import git
import os

import builder.dodo as bos_dodo

from doit.cmd_base import ModuleTaskLoader
from doit.doit_cmd import DoitMain


class CommandManager:
    LOCAL_CONFIGURATION = '.local.yml'

    def __init__(self):
        self._argv = None
        self._args = None
        self._config = None
        self._build_dir = None

    def set_args(self, argv, args):
        self._argv = argv
        self._args = args
        self._config = bos_builder.load_config(args.config)

        # set optional keys to default value
        self._config.setdefault('build.jobs', 1)
        self._config.setdefault('build.verbose', 'no')
        self._config.setdefault('remote.fetch', 'no')
        self._config.setdefault('remote.fetch_always', 'no')
        self._config.setdefault('uenv.mac', 'yes')
        self._config.setdefault('uenv.factory_reset', 'no')
        self._config.setdefault('uenv.sd_images', 'no')
        self._config.setdefault('uenv.sd_boot', 'no')

        # overload settings with local configuration
        if os.path.isfile(self.LOCAL_CONFIGURATION):
            config_local = bos_builder.load_config(self.LOCAL_CONFIGURATION)
            self._config.merge(config_local)

        # change default platform in configuration
        if args.platform:
            self._config.bos.platform = args.platform

        # force default branch for related repository when release branch is active
        # this behaviour cannot be overloaded in local configuration file
        repo_meta = git.Repo(search_parent_directories=True)
        if not repo_meta.head.is_detached:
            repo_branch = repo_meta.active_branch.name
            stable_branch_name = self._config.release.branch.stable
            if repo_branch == stable_branch_name:
                # set all related repositories to stable branch too
                self._config.remote.branch = stable_branch_name

    def _doit_prepare(self, builder, task):
        bos_dodo.builder = builder

        # create build directory for storing doit database
        if not os.path.exists(builder.build_dir):
            os.makedirs(builder.build_dir)

        opt_vals = {'dep_file': os.path.join(builder.build_dir, '.doit.db')}
        commander = DoitMain(ModuleTaskLoader(bos_dodo),
                             extra_config={'GLOBAL': opt_vals})
        commander.BIN_NAME = 'doit'

        logging.info('Preparing LEDE build system...')
        commander.run(['--verbosity', '2', task])

    def get_builder(self, task=None):
        """
        Return bOS builder for current configuration
        """
        builder = bos_builder.Builder(self._config, self._argv)
        if task:
            self._doit_prepare(builder, task)
        return builder

    def prepare(self):
        logging.debug("Called command 'prepare'")
        if self._args.fetch:
            self._config.remote.fetch_always = 'yes'
            self.get_builder('checkout')
        else:
            if self._args.update_feeds:
                self._config.feeds.update_always = 'yes'
            self.get_builder('prepare')

    def clean(self):
        logging.debug("Called command 'clean'")
        builder = self.get_builder('prepare' if not self._args.purge else None)
        builder.clean(purge=self._args.purge)

    def config(self):
        logging.debug("Called command 'config'")
        builder = self.get_builder('prepare')
        builder.config(kernel=self._args.kernel)

    def build(self):
        logging.debug("Called command 'build'")
        if self._args.key:
            keys = self._args.key.split(':', 1)
            key = self._config.setdefault('build.key', bos_builder.ConfigDict())
            key.secret = keys[0]
            key.public = keys[1] if len(keys) > 1 else '{}.pub'.format(keys[0])
        if self._args.jobs:
            self._config.build.jobs = self._args.jobs
        if self._args.verbose:
            self._config.build.verbose = 'yes'

        builder = self.get_builder('prepare')
        builder.build(targets=self._args.target)

    def build_version(self):
        logging.debug("Called command 'build-version'")
        builder = self.get_builder()
        print(builder.get_firmware_version(short=self._args.short))

    def deploy(self):
        logging.debug("Called command 'deploy'")
        # change target MAC address
        if self._args.mac:
            self._config.net.mac = self._args.mac
        # change target hostname and override MAC determination
        if self._args.hostname:
            self._config.deploy.ssh.hostname = self._args.hostname
        # change default pool settings
        if self._args.pool_url:
            scheme, netloc = ([None] + self._args.pool_url.split('://', 1))[-2:]
            server, port = (netloc.rsplit(':', 1) + [None])[:2]
            self._config.miner.pool.host = '{}://{}'.format(scheme, server) if scheme else server
            if port:
                self._config.miner.pool.port = int(port)
        if self._args.pool_user:
            self._config.miner.pool.user = self._args.pool_user
        # change uEnv.txt configuration
        uenv = self._config.uenv
        for option in set(self._args.uenv or []):
            setattr(uenv, option, 'yes')
        # set feeds base index file
        if self._args.feeds_base:
            self._config.deploy.feeds_base = self._args.feeds_base

        # override default targets from command line
        if self._args.target:
            self._config.deploy.targets = bos_builder.ConfigList()
            targets = self._config.deploy.targets
            local = self._config.local
            for target in self._args.target:
                target, path = (target.split(':', 1) + [None])[:2]
                targets.append(target)
                if path:
                    if not target.startswith('local_'):
                        logging.error("Target '{}' cannot have path specification".format(target))
                        raise bos_builder.BuilderStop
                    target = target[6:]
                    if target in ['sd', 'sd_recovery']:
                        setattr(local, target + '_config', path)
                    setattr(local, target, path)

        builder = self.get_builder()
        builder.deploy()

    def status(self):
        logging.debug("Called command 'status'")
        builder = self.get_builder()
        builder.status()

    def debug(self):
        logging.debug("Called command 'debug'")
        builder = self.get_builder()
        builder.debug()

    def toolchain(self):
        logging.debug("Called command 'toolchain'")
        builder = self.get_builder()
        builder.toolchain()

    def release(self):
        logging.debug("Called command 'release'")
        if not self._args.no_fetch:
            # always fetch all repositories before creating release
            self._config.remote.fetch_always = 'yes'

        config_original = bos_builder.load_config(self._args.config)
        builder = self.get_builder('checkout')
        builder.release(self._args.stage, config_original, push=not self._args.no_push, force=self._args.force)

    def key(self):
        logging.debug("Called command 'key'")
        secret = self._args.secret
        public = self._args.public or '{}.pub'.format(secret)

        builder = self.get_builder()
        builder.generate_key(secret_path=secret, public_path=public)


def main(argv):
    command = CommandManager()

    # create the top-level parser
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers()
    subparsers.required = True
    subparsers.dest = 'command'

    # create the parser for the "prepare" command
    subparser = subparsers.add_parser('prepare',
                                      help="prepare source directory")
    subparser.set_defaults(func=command.prepare)
    subparser.add_argument('--fetch', action='store_true',
                           help='force to fetch all repositories')
    subparser.add_argument('--update-feeds', action='store_true',
                           help='force to update all feeds')

    # create the parser for the "clean" command
    subparser = subparsers.add_parser('clean',
                                      help="clean source directory")
    subparser.set_defaults(func=command.clean)
    subparser.add_argument('--purge', action='store_true',
                           help='reset all repositories to its initial state')

    # create the parser for the "prepare" command
    subparser = subparsers.add_parser('config',
                                      help="change default configuration of LEDE project")
    subparser.set_defaults(func=command.config)
    subparser.add_argument('--kernel', action='store_true',
                           help='configure Linux kernel')

    # create the parser for the "build" command
    subparser = subparsers.add_parser('build',
                                      help="build image for current configuration")
    subparser.set_defaults(func=command.build)
    subparser.add_argument('-j', '--jobs', type=int,
                           help='specifies the number of jobs to run simultaneously')
    subparser.add_argument('-v', '--verbose', action='store_true',
                           help='show all commands during build process')
    subparser.add_argument('-k', '--key',
                           help='specify path to build key in a format <secret>[:<public>]; '
                                'when the <public> key is omitted then <secret>.pub is used')
    subparser.add_argument('target', nargs='*',
                           help='build only specific targets when specified')

    # create the parser for the "build-version" command
    subparser = subparsers.add_parser('build-version',
                                      help="get version for current build")
    subparser.set_defaults(func=command.build_version)
    subparser.add_argument('-s', '--short', action='store_true',
                           help='short version without commit suffix')

    # create the parser for the "deploy" command
    subparser = subparsers.add_parser('deploy',
                                      help="deploy selected image to target device")
    subparser.set_defaults(func=command.deploy)
    subparser.add_argument('--mac', nargs='?',
                           help='MAC address of bOS device (it is also used for remote host name determination)')
    subparser.add_argument('--hostname', nargs='?',
                           help='ip address or hostname of remote bOS device with ssh server')
    subparser.add_argument('--pool-url', nargs='?',
                           help='address of pool server in a format <host>[:<port>]')
    subparser.add_argument('--pool-user', nargs='?',
                           help='name of pool worker')
    subparser.add_argument('--uenv', choices=['mac', 'factory_reset', 'sd_images', 'sd_boot'], nargs='*',
                           help='enable some options in uEnv.txt for SD images')
    subparser.add_argument('--feeds-base', nargs='?',
                           help='URL to the Packages file for concatenation with new feeds index '
                                '(for local_feeds target only)')
    subparser.add_argument('target', nargs='*',
                           help='list of targets for deployment (local target can specify also output directory '
                                'in a format <target>[:<path>])')

    # create the parser for the "status" command
    subparser = subparsers.add_parser('status',
                                      help="show status of LEDE repository and all dependent projects")
    subparser.set_defaults(func=command.status)

    # create the parser for the "debug" command
    subparser = subparsers.add_parser('debug',
                                      help="debug application on remote target")
    subparser.set_defaults(func=command.debug)

    # create the parser for the "toolchain" command
    subparser = subparsers.add_parser('toolchain',
                                      help="set environment for LEDE toolchain")
    subparser.set_defaults(func=command.toolchain)

    # create the parser for the "release" command
    subparser = subparsers.add_parser('release',
                                      help="run specific stage of release process")
    subparser.set_defaults(func=command.release)
    subparser.add_argument('--no-fetch', action='store_true',
                           help='do not force fetching all repositories before execution release stage')
    subparser.add_argument('--no-push', action='store_true',
                           help='do not push changes to upstream')
    subparser.add_argument('--force', action='store_true',
                           help='skip some checks and try to force')
    subparser.add_argument('stage', choices=['begin', 'freeze', 'end'], default='begin',
                           help='release process stage')

    # create the parser for the "key" command
    subparser = subparsers.add_parser('key',
                                      help="generate build key pair for signing firmware tarball and packages")
    subparser.set_defaults(func=command.key)
    subparser.add_argument('secret',
                           help='path to secret key output')
    subparser.add_argument('public', nargs='?',
                           help='path to public key output; when omitted then <secret>.pub is used')

    # add global arguments
    parser.add_argument('--log', choices=['error', 'warn', 'info', 'debug'], default='info',
                        help='logging level')
    parser.add_argument('--config', default=bos_builder.DEFAULT_CONFIG,
                        help='path to configuration file')
    parser.add_argument('--platform', choices=['zynq-dm1-g9', 'zynq-dm1-g19', 'zynq-dm1-g29', 'zynq-am1-s9'], nargs='?',
                        help='change default bOS platform')

    # parse command line arguments
    args = parser.parse_args(argv)

    # create color handler
    handler = colorlog.StreamHandler()
    handler.setFormatter(colorlog.ColoredFormatter(log_colors={
        'DEBUG':    'cyan',
        'INFO':     'green',
        'WARNING':  'yellow',
        'ERROR':    'red',
        'CRITICAL': 'red,bg_white',
    }))

    # set logging level
    logging.basicConfig(level=getattr(logging, args.log.upper()), handlers=[handler])

    # set arguments
    command.set_args(argv, args)

    # call sub-command
    args.func()


if __name__ == "__main__":
    # execute only if run as a script
    try:
        main(sys.argv[1:])
    except bos_builder.BuilderStop:
        sys.exit(1)
