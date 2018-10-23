# Braiins OS Developer Guide

This project is build around a small tool (braiins build system - bb.py) that extends the OpenWrt build system with following features:

* automatic clone/pull of all repositories
* switching between several configurations
* out-of-tree repositories for development
* status of all developed repositories
* cleaning and purging repositories
* setup environment for using the OpenWrt toolchain in external projects
* firmware deployment to NAND/SD over ssh connection or to local repository
* release version control and firmware signing
* preparation of feed server with firmware updates

## Getting Started

These instructions will get you a copy of the project up and running on your local machine for development and testing
purposes. See deployment for notes on how to deploy the project on a live system.

### Prerequisites

The build system is written for *Python 3.5*. The only prerequisites are:

* Python 3.5.x
* virtualenv 15.x.x
* pip3 9.x.x

### Installing

Clone git repository to some directory and setup system environment:

```bash
# clone braiins OS from git
$ git clone https://github.com/braiins/braiins-os.git
$ cd braiins-os
$ virtualenv --python=/usr/bin/python3.5 .env
$ source .env/bin/activate
$ pip3 install -r requirements.txt
```

## Building

Building firmware image with default configuration is very simple. Just call the *bb.py* script with *build* command.

```bash
# build firmware image with default configuration
$ ./bb.py build

# build firmware image with user configuration
$ ./bb.py --config configs/user.yml build
```

All repositories are stored in **build**/*\<target\>* directory where *target* is specified in *YAML* configuration file
under a *build.name* attribute.

### Platform Selection

The build system supports multiple platforms with the same base configuration. Currently the following platforms
are supported:

* *zynq-am1-s9* (Antminer S9)
* *zynq-dm1-g9* (DragonMint v1 with G9 Control Board)
* *zynq-dm1-g19* (DragonMint v1 with G19 Control Board)

The platform **zynq-dm1-g19** is specified in the default configuration file but it can be modified from command line
with a *platform* parameter:

```bash
# build firmware image with default configuration for DragonMint G9
$ ./bb.py --platform zynq-dm1-g9 build

# build firmware image with user configuration for DragonMint G19
$ ./bb.py --config configs/user.yml --platform zynq-dm1-g19 build
```

### Firmware Release

The firmware with specific version has tag in a git repository which contains modified configuration set to exact commit
of all dependent repositories. The tag can be checked out for specific firmware version. Afterwards, we can call *build* command
for reproducible firmware release.

### Signing

By default the resulting firmware image and packages are signed by a test key which is specified in the default config
by a *build.key* attribute and is stored in *keys* directory. The release key is usually stored in a fortified keyring
and is securely used during actual release.

To set this key without changing the configuration file it is possible to use a *key* parameter of the *build* command:

```bash
# use secret key for signing (the public key is '/path/secret.pub')
$ ./bb.py build --key /path/secret

# secret and public key can be specified in one parameter
$ ./bb.py build --key /path/secret:/path/public
```

## Development

### Fetching

Default configuration on master branch has disabled automatic fetching/merging of remote repositories when *build*
command is executed. The *prepare* command can be used instead when synchronization with remote is needed.

```bash
# force fetching from remote repositories
$ ./bb.py prepare --fetch
```

### Cleaning

It is possible to clean all projects with two options. Simple execution of *clean* command runs the OpenWrt *make clean* to
clean the whole build system. It does not guarantee that all files will be in its initial state.

The second option uses git command to clean all repositories. The command after clean also runs initialization phase
again and prepares repository for its first build. This option removes all untracked files and must be called with
caution!

```bash
# clean repositories with the OpenWrt make clean
$ ./bb.py clean

# reset repositories with git clean
$ ./bb.py clean --purge
```

### Status

The braiins OS project consists of several git repositories. It is convenient to track status of all changes in all repositories at once
during development. The *status* command can be used for this purpose. It is similar to git status but it is executed for all repositories.

```bash
# get status of all repositories
$ ./bb.py status
```

### Out-of-Tree Build

Rather than executing the whole OpenWrt build system which can be slow, we can run a separate build of subproject (e.g.
CGMiner) with the OpenWrt toolchain. Environment variables must be set correctly for using the OpenWrt toolchain in
out-of-tree projects. For this purpose, the *toolchain* command is provided.

```bash
# set environment variables for OpenWrt toolchain out-of-tree use
$ eval $(./bb.py toolchain 2>/dev/null)
```


## Configuration

The build system supports multiple configurations specified by a configuration file stored in a YAML format. The
current configuration can be changed from a command line. From the command line, it is also possible to alter the most
important parameters without modifying the underlying configuration file.

The configuration is divided into two categories. The first one is target specific configuration which is handled
exclusively by the braiins build system and can be adjusted only in the YAML configuration. The second one is a package
configuration used for image content description which is handled mainly in the OpenWrt menuconfig.

### YAML Structure

The main configuration file is stored in a standard [YAML 1.2][1] format. The format expects predefined hierarchical
structure which is formed by categories on the global level. The categories can be further divided into subcategories or
they can directly contain configuration attributes.

The string attributes can use special syntax for parameter expansion which is extension of standard YAML format. The
name of parameter for expansion is enclosed in *{}* and can be used anytime in the string. The list of supported
parameters is following:

* *platform* - the name defined in a *miner.platform* attribute (it has form *\<target\>-\<subtarget\>*)
* *target* - the name of target architecture (e.g. *zynq*) that is derived from *miner.platform* attribute
* *subtarget* - the name of target device (e.g. *dm1-g19*) that is derived from *miner.platform* attribute
* *subtarget_family* - the name of the family of the subtarget (e.g. *dm1*) that is derived from *miner.platform*
* *build_dir* - build directory e.g. *build/\<target\>*

Curly bracket is also used by the YAML for dictionary in an abbreviated form and when string starts with the curly
bracket then it must be quoted to distinguish meaning:

```yaml
# use quotes when string starts with { 
name: '{target}'

# form without quotes when it is not ambiguous
sd: output/{platform}/sd/
```

The default configuration file is fully commented so the following list of global categories is only short description:

* *miner* - the settings concerning one instance of miner (platform, MAC, HWID, default pool); default configuration is
  used only for testing and is usually overridden from command line during release process
* *build* - the configuration of build process (path to OpenWrt configuration, build directories, keys, ...)
* *remote* - the list of all remote repositories with parameters for fetching; the parameters *fetch* and *branch* used
  as a default value for all repositories could be overridden in a specific repository by parameter of the same name
* *local* - the configuration of output directories for local targets for deployment
* *feeds* - the settings of feeds fetching and installation
* *uenv* - the configuration of *uEnv.txt* content (this file is used only for SD images)
* *deploy* - the list of targets for deployment and configuration of this process (e.g. reset of target environment,
  remote ssh connection, ...)

### CLI Parameters

The braiins build system supports multiple configurations which can be selected by global parameter *--config*. When the
script is run without this parameter, **configs/default.yml** is used. The *--platform* parameter can be used for
changing the target platform.

*Global configuration parameters must be consistently used with all commands to guarantee predictable results!*

The build system commands are described in detail in separate sections. Below is a list of supported commands:

* *prepare* - fetch all remote repositories and prepare source directory
* *clean* - clean source directory
* *config* - change default configuration of OpenWrt project
* *build* - build image for current configuration
* *deploy* - deploy selected image to target device (NAND/SD over ssh or to local directory)
* *status* - show status of all local repositories (*git status* equivalent)
* *toolchain* - set environment for OpenWrt toolchain (out-of-tree build)
* *release* - create branch with configuration for release version
* *key* - generate build key pair for signing firmware tarball and packages

### Packages

The standard OpenWrt menuconfig is used for firmware image configuration. When some changes are detected, the difference in
configuration is saved to the file specified in *YAML* configuration file under *build.config* attribute.

```bash
# configure image packages
$ ./bb.py config
```

Multiple firmware images are being built at once (NAND, NAND Recovery, SD, ...). We must be specify which image will
contain a particular package. It is done in two ways:

- When a package is installed to all images without exception then only OpenWrt menuconfig is used where the package must
be selected by asterisk symbol `<*>`
- When a package is installed only to specific images then the package must be selected as a module `<M>` and added to
an external package list specified in a *build.packages* attribute.

The package file is just another YAML structured format that stores lists with inheritance support. The lists with
*image_* prefix are used for description of installed packages in specified image:

* *image_sd* - SD image with extroot support (second partition in the ext4 format is used as an overlay)
* *image_nand* - standard NAND image
* *image_recovery* - special NAND recovery image (it also supports factory reset)
* *image_upgrade* - NAND image for generic stage1 upgrade process from different firmwares

The structured list has the following format:

```yaml
list_name:
  # inheritance is specified as a list of base lists
  # root list has this parameter omitted
  base:
    - child1
    - child2
  # the list items are specified under separate parameter
  # the resulting list is merged with base lists in order:
  # child1.list, child2.list, item1, item2
  list:
    - item1
    - item2
```

### Kernel

The *config* command can also be used for the Linux configuration when *--kernel* parameter is specified. The resulting
configuration is then saved in the OpenWrt build system in the target directory. It is standard behavior of the OpenWrt.

```bash
# configure kernel (Linux) for selected target
$ ./bb.py config --kernel
```

## Deployment

Whenever firmware images are built by the OpenWrt build system, it is possible to deploy them over ssh connection directly
to the running machine (when it runs compatible firmware) or store it to a local path. The default configuration builds
all local targets and stores its result to predefined location **output**/*\<platform\>*. It is convenient for testing
when we want to verify all possible targets. However, for real deployment, it is more useful to specify a target from
the command line.

### System Upgrade vs. Deployment

*Do not confuse deployment process with the system upgrade!* The deployment is used mainly for developers for testing
the firmware on running miner or for initial factory NAND programming. For system upgrade use standard firmware tarball
which can be loaded with help of web interface or with OpenWrt *sysupgrade* utility.
Follow [user manual](docs/user-manual) for standard firmware upgrade procedure

```bash
# download latest packages from feeds server
$ opkg update
# try to upgrade to the latest firmware
$ opkg install firmware
```

### Remote Targets

Only commonly used remote targets will be described here. Special targets, useful during development of specific
firmware parts, will be omitted. With remote targets, it is possible to deploy either NAND image or SD image (in case
that the SD card is inserted into the SD slot). The NAND image can be deployed even if the braiins OS is run from NAND and a
UBI partition is mounted. The following targets are supported:

* *sd* - writes U-Boot and Linux image with a *SquashFS* root file system to the SD card
* *nand* - writes U-Boot and UBI image with the Linux kernel and a *SquashFS* root file system to the NAND (the writable
  overlay uses a *UBIFS* file system)

Let's assume local network with one instance running braiins/OpenWrt firmware and default configuration of the build system.
The following command can be used for deployment of SD or NAND image to this machine:

```bash
# mount mmc0 partition 1 and copy all images and 'uEnv.txt' to it
$ ./bb.py deploy sd

# write U-Boot, recovery image and configuration to NAND and do factory reset
$ ./bb.py deploy nand
``` 

When more than one device needs to be managed, several arguments can be used to specify remote machine. It can be done only
by machine's MAC address specification or even with a hostname when local DNS server does not work correctly or when the MAC
address does not correspond with the hostname.

*Be very cautious with MAC address!* If the *--mac* parameter is omitted, the default MAC address from configuration
file is used (`00:0A:35:FF:FF:FF`) and remote machine is upgraded with it. Therefore, it is recommended to use hostname
only in situations when miners MAC address needs to be changed.

The hostname is determined from MAC address when not specified. The machine generates its name based on current MAC in a form of
`{MACHINE_CLASS}-xxyyzz` where `MACHINE_CLASS` is e.g. `miner` and `xxyyzz` are last three numbers from its address.

```bash
# upgrade remote miner with the hostname 'miner-ffff01'
$ ./bb.py deploy nand --mac 00:0A:35:FF:FF:01
# upgrade remote miner on address '192.168.0.1' and change its MAC to '00:0A:35:FF:FF:FF'
$ ./bb.py deploy nand --hostname 192.168.0.1
# upgrade previous miner and set its MAC to original value
$ ./bb.py deploy nand --mac 00:0A:35:FF:FF:01 --hostname miner-ffffff
```

There are also special configuration sub-targets which modify only miner configuration and do not touch other parts of
the NAND or SD partition:

* *sd_config* - modify only *uEnv.txt* file on SD card which is read by the U-Boot
* *nand_config* - modify only NAND U-Boot environment and miner configuration partition

### Local Targets

Local targets can be used for deploying images to locations specified by a file path. The default configuration enables
all local targets for storing all images to a predefined directory **output**/*\<platform\>*. There are also special
local targets for deployment utilities used for upgrading the original firmware to the braiins/OpenWrt one. The other
special target is for a feeds server preparation used for upgrading braiins/OpenWrt firmware with a standard OpenWrt *opkg*
utility. The following list specifies main local targets:

* *local_sd* - the same function as remote target but target is specified by a local file path
* *local_sd_recovery* - writes special SD recovery image to a local file path (e.g. it can be used for repairing a
  'bricked' machine that doesn't boot from its flash memory anymore)
* *local_nand_dm_v1* - scripts and images needed for upgrading an original DragonMint firmware
* *local_nand_dm_v2* - scripts and images needed for upgrading an improved DragonMint firmware (Kolivas)
* *local_nand_am* - scripts and images needed for upgrading Antminer S9 factory firmware
* *local_feeds* - sysupgrade tarball with current firmware and packages needed for creating standard OpenWrt feeds server

Similarly to the remote targets there are also *configuration* targets:

* *local_sd_config* - modify only *uEnv.txt* file (useful for changing parameter *sd_boot*)
* *local_sd_recovery_config* - modify only *uEnv.txt* file (useful for changing parameters *sd_boot*, *factory_reset*
  and *sd_images* controlling SD recovery image for factory reset)

The output location is usually specified by the command line. Since more than one target can by specified at once there
is special notation for passing local file path to the specific local target:

```
<local_target>[:<path>]
```

Miner MAC address can also be specified with *--mac* parameter. However, it is only used for generating the *uEnv.txt*.
This MAC address is used when booting the miner from an SD card. The *--hostname* parameter is ignored for local
targets. There are several useful parameters for miner configuration which will be described in the next section.

Below are a few typical examples of *deploy* command for local targets:

```bash
# create SD card with default MAC address without SD boot parameter
# a hardware jumper on control board have to be connected to boot from this SD card
$ ./bb.py deploy local_sd:/mnt/mmc0

# create SD card with MAC address '00:0A:35:FF:FF:01' and with SD boot enabled
# it can boot from SD card without connecting a hardware jumper if compatible U-Boot is used
$ ./bb.py deploy local_sd:/mnt/mmc0 --mac 00:0A:35:FF:FF:01 --uenv sd_boot

# create recovery SD card which boots from SD and performs NAND factory reset using images stored on this SD 
$ ./bb.py deploy local_sd_recovery:/mnt/mmc0 --mac 00:0A:35:FF:FF:01 --uenv sd_boot factory_reset sd_images

# create special SD card only with 'uEnv.txt' which performs factory reset when it is inserted in a miner
$ ./bb.py deploy local_sd_config:/mnt/mmc0 --uenv factory_reset
```

### uEnv

When U-Boot finds inserted SD card it tries to load a file *uEnv.txt* from its first partition formatted with FAT
file system. There are environment variables which can alter U-Boot behavior during boot process. There are
standard U-Boot variables (e.g. ethaddr) and some additional ones are provided by braiins/OpenWrt firmware. Configuration of these
variables can be done in the braiins build system YAML file in *uenv* section. These parameters can also be passed by
command line argument *--uenv*. The following list shows all supported settings:

* *mac* - set miner MAC address (generates *ethaddr* variable)
* *factory_reset* - when SD has this variable enabled and is inserted into the miner, the miner performs factory reset
* *sd_images* - used for factory reset images from SD (*factory_reset* must also be enable)
* *sd_boot* - boot kernel image from SD (the U-Boot is still booted from the NAND)

The *sd_boot* requires compatible and functional U-Boot on NAND. When the NAND is corrupted it may not work. In
that case a HW jumper must be used for a miner control board reconfiguration. E.g. *J2* pins must be bridged on
G9/G19 boards to change boot mode from NAND to SD card.

### Default Pool

Each miner with the same firmware can store different default pool. The information is stored in a miner configuration
partition in the NAND. For SD version this functionality is not currently supported. The default pool can be changed
from command line with corresponding arguments of deploy command. However, these arguments have effect only for remote
targets and for special local targets for an original firmware upgrade. The deploy command supports the following
arguments:

* *pool-url* - the address of pool server in a format *\<host\>[:\<port\>]*<br>
  (*stratum+tcp://stratum.slushpool.com:3333*)
* *pool-user* - the name of user and worker<br>
  (*braiinstest.worker1*)

## Release Management

The braiins build system also has tools for firmware versioning which is used in release cycles. It is based on
git repository with tags which holds name of a firmware version and configuration for reproducible firmware build. The
release cycle has three stages:

1. new version creation,
2. signed firmware building,
3. publication.

### Versioning

The first stage is about git branch creation, modification of default configuration file where each repository points to
specific commit and tag creation with a name representing current firmware version. All this can be done by one command
with a name *release*. This command requires that the braiins build system repository and all dependent repositories are
clean. After successful call of this command, a *remote* tag is created with the following version format:

```
firmware_<YYYY-MM-DD>-<patch_level>-<short_sha>
```

The `<YYYY-MM-DD>` represents a *date* of the braiins build system *commit* from which is a release created. The value of
the `<patch_level>` is usually 0 and is incremented only in situation when more then one release is created in one day.
This increment is done automatically and depends on correctly created git tags. The `<short_sha>` is a SHA prefix
of the *commit* used for the date. The prefix is 8 characters long.

The *release* command has also *--include* argument which is used for specification of a firmware tarball
content. In a special situation that a new firmware needs to upgrade also a U-Boot or a FPGA
bitstream. Occasionally, a bash script (*COMMAND*) can also be added. It is run before in pre-init phase of the
standard system upgrade process. It can contain some control checks or fixes of previous firmware running on a
miner. The source code of this script is stored in the OpenWrt repository but must be configured externally that it is
included to the output image. The following list contains all sysupgrade components supported by the firmware:

* *command* - bash script executed during firmware system upgrade
* *uboot* - the U-Boot image for upgrading previous one (it can brick the miner)
* *fpga* - the FPGA bitstream (the miner has auto recovery process which can rescue a miner when the new bitstream does
  not work)

```bash
# create git tag and push it to the remote repository
$ ./bb.py release

# do the same but also include 'COMMAND' script and new FPGA bitstream
$ ./bb.py release --include command fpga
```

### Building and Signing 

The official firmware is signed with publisher key which should be private. Only one key should exist and be stored
in some secured keyring. The key can be generated by the braiins build system with the following command:

```bash
# generate key pair and store it to the fortified keyring
$ ./bb.py key ~/keyring/secret
```

This command generates private and public key into the specified path. Where the private key is to be securely
stored is beyond the scope of this description. This key is usually generated only once and is used for signing of
all the releases firmwares.

After the release has been created with the *release* command, it can be built and signed with the following command:

```bash
# switch braiins build system to specific firmware version
$ git checkout firmware_2018-05-27-0-16a21b55
# build this version and sign it with a secret key
$ ./bb.py build --key ~/keyring/secret
```

If everything goes well, all images are prepared for final publishing to the feeds server. This process can be
reproduced anytime in the feature.

### Feeds Server

The final stage of release management is publishing to the feeds server. It is standard OpenWrt feeds server with the
*Packages.gz* file containing list of *ipk* packages in a text format. All files needed for this feed server can be
created by *deploy* command with *local_feeds* target:

```bash
# initial feeds server is created by deploy command with 'local_feeds' target
$ ./bb.py deploy local_feeds:~/server/initial_feeds

# the other deployments should be created with the previous contents
$ ./bb.py deploy local_feeds:~/server/new_feeds --feeds-base ~/server/initial_feeds/Packages
```

The output directory should be empty before calling deploy command to ensure that the directory would not contain any
temporary files. If feeds server contains previous firmwares too the *--feeds-base* should be called to merge previous
*Packages* index file with new firmware. The previous *Packages* index file can also be edited before new deployment to
prune some old firmwares from the server.

All generated files are described in the following list:

* **firmware_\<version\>.tar** - signed tarball with all images for miner system upgrade compatible with *sysupgrade*
  utility or LuCI web interface (this file can be used directly without *OPKG* utility)
* **firmware_\<version\>.ipk** - standard *OPKG* package with firmware metadata used for installing new firmware<br>
  (it downloads corresponding *firmware_\<version\>.tar* from feeds server and initiate system upgrade)
* **Packages** - feeds index file with a list of all packages in a text form<br>
  (it contains references to *firmware_\<version\>.tar*)
* **Packages.gz** - gzipped *Packages* file
* **Packages.sig** - the file that contains signature for *Packages.gz*

## Upgrade from Original/Factory Firmware

The example below for Dragon Mint DM1 shows how to upgrade the factory firmware to braiins OS firmware:

```bash
# create stage1 upgrade script and all required images for new DragonMint with G19 control board
$ ./bb.py deploy local_nand_dm_v2:~/nand_dm_v2

# run generated upgrade script from local host and initiate upgrade over ssh connection
$ cd ~/nand_dm_v2
$ python3 ./upgrade.py 192.168.0.1
```

There exists two versions of original firmware and appropriate target for deploy should be used:

* *local_nand_dm_v1* - first version of firmware which running only *telnet* server
* *local_nand_dm_v2* - improved version which uses *ssh* server instead

You have to get login information for *root* access over *telnet* (v1) or *ssh* (v2) for your DragonMint miner before
you start the upgrade process. Without this information you have to open your miner and use SD version for boot and
deploy this firmware with the braiins build *deploy* command with *nand* target.

## Authors

* **Libor Vašíček** - *Initial work*

## License

It is released under the GNU General Public License v3.0 ([GNU GPLv3][2]).

See LICENSE file.

[1]: http://yaml.org/spec/1.2/spec.html
[2]: https://choosealicense.com/licenses/gpl-3.0/
