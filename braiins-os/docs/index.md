# Table of contents

   * [Overview](#overview)
   * [Braiins OS Versioning Scheme](#braiins-os-versioning-scheme)
   * [Installing Braiins OS for the First Time (Replacing Factory Firmware with Braiins OS)](#installing-braiins-os-for-the-first-time-replacing-factory-firmware-with-braiins-os)
      * [Initial Steps](#initial-steps)
      * [Phase 1: Creating Bootable SD Card Image (Antminer S9 example)](#phase-1-creating-bootable-sd-card-image-antminer-s9-example)
      * [Phase 2: Permanently Migrating from Factory Firmware to Braiins OS](#phase-2-permanently-migrating-from-factory-firmware-to-braiins-os)
         * [Option A: Install from SD card](#option-a-install-from-sd-card)
         * [Option B: Install using SSH](#option-b-install-using-ssh)
   * [Basic user's guide](#basic-users-guide)
      * [Miner Signalization (LED)](#miner-signalization-led)
         * [Recovery Mode](#recovery-mode)
         * [Normal Mode](#normal-mode)
         * [Identifying a miner in a farm](#identifying-a-miner-in-a-farm)
      * [Fan control](#fan-control)
         * [Fan operation](#fan-operation)
         * [Default temperature limits](#default-temperature-limits)
         * [Fan Control configuration in cgminer.conf](#fan-control-configuration-in-cgminerconf)
      * [AsicBoost support](#asicboost-support)
      * [Migrating from Braiins OS to factory firmware](#migrating-from-braiins-os-to-factory-firmware)
      * [Recovering Bricked (unbootable) Devices Using SD Card](#recovering-bricked-unbootable-devices-using-sd-card)
      * [Firmware upgrade](#firmware-upgrade)
      * [Network miner discovery](#network-miner-discovery)
         * [IP Report/Set button](#ip-reportset-button)
         * [Discover scipt](#discover-scipt)
      * [Batch migration to Braiins OS](#batch-migration-to-braiins-os)
      * [Setting miner password via SSH](#setting-miner-password-via-ssh)
      * [Reset to initial Braiins OS version](#reset-to-initial-braiins-os-version)
      * [Recovery Mode](#recovery-mode-1)
      * [Cloning the Braiins OS repository](#cloning-the-braiins-os-repository)

# Overview

This document is a quick-start guide on how to install Braiins OS on your mining device.
There are two ways to test and use Braiins OS:

1. **Boot from SD card** with Braiins OS image, effectively keeping the stock firmware in the built-in flash memory. In case you encounter any issues, you can simply boot the stock firmware from the internal memory. This is a safe method we suggest to start with.

2. **Permanently reflash the stock firmware**, effectively replacing the manufacturer’s firmware completely with Braiins OS. In this method, the only way to go back to the default stock setup is to restore the manufacturer’s firmware from a backup that you create during install.

Due to the aforementioned reasons, it is highly recommended to install Braiins OS firmware **only on devices with SD card slots**.

You will need:

* supported ASIC miner (see the list below)
* computer with Linux or Windows
* SD card (optional but recommended method)

*Note: Commands used in this manual are for instructional purposes. You might need to adjust file paths and names appropriately.*

# Braiins OS Versioning Scheme

Each release contains a version number that consists of YYYY-MM-DD-P, where:

| Field | Meaning |
| --- | --- |
| YYYY | 4 digit release year |
| MM | 2 digit month |
| DD | 2 digit day |
| P | single digit patch level - in case there was a hotfix re-release on the same day |

The version number is also encoded in all artifacts that are available for download.

In addition to the above, each major Braiins OS release has a code name (e.g. wolfram, cobalt, etc.).

## Transitional firmwares

The table below outlines the correspondence between transitional firmware image archives (as provided at [https://feeds.braiins-os.org/](https://feeds.braiins-os.org/)) and a particular hardware type.

| Firmware prefix | Hardware |
| --- | --- |
| braiins-os_am1-s9_*.tar.gz | Antminer S9, S9i, S9j; (**R4** support is **broken** do not **USE**!!) |
| braiins-os_dm1-g29_*.tar.gz | Dragon Mint T1 with **G19** or **G29** control board *|

*NOTE: transitional firmware for Dragonmints T1 with **G19** control board is no longer provided as it is fully compatible with **G29** transitional firmware.

# Installing Braiins OS for the First Time (Replacing Factory Firmware with Braiins OS)

The steps described below only need to be done **the very first time** you are installing Braiins OS on a device. You will be using so-called *transitional firmware images* mentioned above for this purpose.

## Initial Steps

Download the latest release of transitional firmware images and GPG signatures from the feeds site at: [https://feeds.braiins-os.org/](https://feeds.braiins-os.org/)


You can check the downloaded file for its authenticity and integrity. The image signature can be verified by [GPG](https://www.gnupg.org/documentation/):

```bash
curl https://slushpool.com/media/download/braiins-os.gpg.pub | gpg2 --import
for i in ./braiins-os_*asc; do gpg2 --verify $i; done
```

You should see something like:

```
gpg: assuming signed data in './braiins-os_am1-s9_2018-10-24-0-9e5687a2.tar.gz'
.
.
gpg: Good signature from "Braiins Systems Release Key (Key used for signing software made by Braiins Systems) <release@braiins.cz>" [ultimate]
```

Unpack the selected (or all) transitional firmware images using standard file archiver software (e.g. 7-Zip, WinRAR) or the following command (Linux):

```bash
for i in  ./braiins-os_*.tar.gz; do tar xvjf $i; done
```

### Transitional firmware image types

The table below explains the use of individual transitional firmware images

| Firmware prefix | Hardware |
| --- | --- |
| braiins-os_HARDWARE_TYPE_**sd**_VERSION.img | SD card image for testing on hardware, recovering a *bricked* machine, etc. |
| braiins-os_HARDWARE_TYPE_**ssh**_VERSION.tar.gz | transitional firmware for upgrading from factory firmware that has **ssh** access |
| braiins-os_HARDWARE_TYPE_**telnet**_VERSION.tar.gz | transitional firmware for upgrading from factory firmware that provides **telnet** access |
| braiins-os_HARDWARE_TYPE_**web**_VERSION.{vendor specific extension} | transitional firmware for upgrading from factory firmware via the **factory firmware web interface**. The exact file extension depends on particular hardware type |


## Phase 1: Creating Bootable SD Card Image (Antminer S9 example)

Insert an empty SD card (with a minimum capacity of 32 MB) into your computer and flash the image onto the SD card.

### Using software with GUI (Windows, Linux)

* [Download](https://etcher.io/) and run Etcher
* Select the sd.img image
* Select drive (SD card)
* Copy the image to SD card by clicking on *Flash!*


### Using bash (Linux)

Identify SD cards block device (e.g. by ```lsblk```) and run the following commands (replacing ```VERSION``` with the most recent release):

```
sudo dd if=braiins-os_am1-s9_sd_VERSION.img of=/dev/your-sd-card-block-device
sync
```

### Booting the Device from SD Card
- Unmount the SD card
- Adjust jumper to boot from SD card (instead of NAND memory):
   - [Antminer S9](s9)
   - [Dragon Mint T1](dm1)
- Insert it into the device, then start the device.
- After a moment, you should be able to access the Braiins OS interface through the device's IP address.

### MAC & IP address

By default, the device's MAC address stays the same as it is inherited from firmware (stock or bOS) stored in the device (NAND). That way, once the device boots with Braiins OS, it will have the same IP address as it had with the factory firmware.

Alternatively, you can specify a MAC address of your choice by modifying the `ethaddr=` parameter in the `uEnv.txt` file (found in the first FAT partition of the SD card).

### Using single SD card on multiple devices

The most recently used MAC address is stored on the SD card overlay partition to check if the SD is inserted to the same device. If the current MAC address differs from the previous one, then the network and system configuration is reset to its default and `/etc/miner_hwid` is deleted.

HW_ID is determined from NAND if it stores bOS FW. If NAND is corrupted or it contains stock firmware, then the file `/etc/miner_hwid` is used if it exists, otherwise a new HW_ID is generated and stored to `/etc/miner_hwid` to preserve HW_ID until the next boot.

## Phase 2: Permanently Migrating from Factory Firmware to Braiins OS

Once the SD card works, it is very safe to attempt flashing the built-in flash memory.

### Option A: Install from SD card

In the System > Install Current System to Device (NAND) menu, you can trigger an operation that writes Braiins OS permanently to the miner's NAND memory. Please note that a backup of the original firmware is NOT created during the process.

Alternatively, you can login to the SD card running the miner via SSH and run the following command: ```miner nand_install```

When NAND contains original or bOS firmware, the MAC address and other network information are preserved for newly installed firmware. The device settings installed to NAND cannot be overridden by changing network settings in a system running on a SD card.

Once done, do not forget to adjust your jumper to boot from NAND memory (instead of the SD card):
   - [Antminer S9](s9)
   - [Dragon Mint T1](dm1)

### Option B: Install using SSH

*Note: You must have Python 3 installed in order to install Braiins OS using SSH and run all the `*.py` scripts. [Follow this guide to install Python 3.](python-install)*


The tool creates a backup of the original firmware in the ```backup``` folder. It is important to **keep the backup safe** to resolve any potential future issues.

Below are commands to replace original factory firmware with Braiins OS using the SSH variant. The tool attempts to login to the machine via SSH, therefore you may be prompted for a password.

#### Using Linux

```bash
cd braiins-os_am1-s9_ssh_VERSION
virtualenv --python=/usr/bin/python3 .env
source .env/bin/activate
python3 -m pip install -r requirements.txt

python3 upgrade2bos.py your-miner-hostname-or-ip
```

#### Using macOS

```bash
cd braiins-os_am1-s9_ssh_VERSION
virtualenv venv
source venv/bin/activate
pip install -r requirements.txt

python3 upgrade2bos.py your-miner-hostname-or-ip
```

#### Using Windows

```bash
cd braiins-os_am1-s9_ssh_VERSION

# create bos environment if it does not exist
mkvirtualenv bos
setprojectdir .

# select bos environment if it has been created already
workon bos

pip install -r requirements.txt

python upgrade2bos.py your-miner-hostname-or-ip
deactivate
```

#### Adding a post-upgrade script

There is an option `--post-upgrade` to run a custom post upgrade script.

```
python3 upgrade2bos.py your-miner-hostname-or-ip --post-upgrade path-to-directory-with-script
```

The directory with post-upgrade script must contain stage3.sh which is run after miner successfully upgrades. This directory can contain other files which can be accessed from the stage3.sh in the scope of the current directory when the script is run.

#### Example script (change default login)

The content of `stage3.sh`:

```
#!/bin/sh

# set new password for web interface
echo -e "[newpassword]\n[newpassword]" | passwd

# disable SSH password authentication
uci set dropbear.@dropbear[0].PasswordAuth='off'
uci set dropbear.@dropbear[0].RootPasswordAuth='off'
uci commit dropbear

# copy SSH authorized keys for password-less login
cp ./authorized_keys /etc/dropbear/
```

The content of `authorized_keys` is in a standard format specified by Dropbear or OpenSSH server. The keys can be obtained from the local machine:

```
cp ~/.ssh/id_rsa.pub >> ./authorized_keys
```

# Basic user's guide

## Miner Signalization (LED)

Miner LED signalization depends on its operational mode. There are two modes (*recovery* and *normal*) which are signaled by the **green** and **red LEDs** on the front panel. The LED on the control board (inside) always shows the *heartbeat* (i.e. flashes at a load average based rate).

### Recovery Mode

Recovery mode is signaled by the **flashing green LED** (50 ms on, 950 ms off) on the front panel. The **red LED** represents access to a NAND disk and flashes during factory reset when data is written to NAND.

### Normal Mode

The normal mode state is signaled by the combination of the front panel **red** and **green LEDs** as specified in the table below:

| red LED | green LED | meaning |
| ------- | --------- | ------- |
| on | off | *cgminer* or *cgminer_monitor* are not running |
| slow flashing | off | hash rate is below 80% of expected hash rate or the miner cannot connect to any pool (all pools are dead) |
| off | very slow flashing (1 sec on, 1 sec off) | *miner* is operational and hash rate above 80 % of expected hash rate |
| fast flashing | N/A | LED override requested by user (`miner fault_light on`) |

### Identifying a miner in a farm

The local miner utility can also be used to identify a particular device by enabling aggressive blinking of the **red LED**:

```bash
$ miner fault_light on
```

Similarly to disable the LED run:

```bash
$ miner fault_light off
```

## Fan control

Braiins OS supports automatic fan control for both T1 and S9 miners (using [PID controller](https://en.wikipedia.org/wiki/PID_controll)). The fan controller can operate in one of two modes:

- **Automatic Fan Control** - Miner software tries to regulate the fan speed so that miner temperature is approximately at the target temperature (which can be configured). The allowed temperature range is 30-90 degree Celsius.
- **Fixed fan speed** - Fans are kept at a fixed, user-defined speed, no matter the temperature. This is useful if you have your own way of cooling the miner or if the temperature sensors don't work. Allowed fan speed is 0%-100%.

The fan control mode can be inspected in the "Miner Stats" page and changed in the "Miner Configuration" page.

**Warning**: misconfiguring fans (either by turning them off or to a level that is too slow, or by setting the target temperature too high) may irreversibly **DAMAGE** your miner.

The default behavior for fan control is to be in "Automatic Fan Control" mode with a reasonably set target temperature. The number and type of temperature sensors is different on T1 and S9, but in general the "measured temperature" is the maximum of temperatures from all sensors.

### Fan operation

1. The miner's starting fan speed is automatically set to 100% no matter the configuration (this is to prevent overheating during initialization).
2. Once temperature sensors are initialized, fan control is enabled. If temperature sensors are not working or they read out temperature of 0, fans are automatically set to full speed.
3. If the current mode is "fixed fan speed", the fan is set to a given speed.
4. If the current mode is "automatic fan control", the fan speed is regulated by temperature. The minimum fan speed is set to 60% for the first 2 minutes (to wait for the miner to warm up), after which time the minimum speed is 10%.
5. In case miner temperature is above *HOT temperature*, fans are set to 100% (even in "fixed fan speed" mode).
6. In case miner temperature is above *DANGEROUS temperature*, cgminer shuts down (even in "fixed fan speed" mode).

### Default temperature limits

Miner type | Default Target Temperature | *HOT temperature* | *Dangerous temperature*
--- | --- | --- | ---
S9 | 75&deg;C | 90&deg;C | 95&deg;C
T1 | 89&deg;C | 95&deg;C | 105&deg;C


The 'hot' and 'dangerous' temperature limits can be overriden in ```/etc/cgminer.conf``` e.g.:

```
fan-hot-temp: "91"
fan-dangerous-temp: "96"
```

These options have no representation in the web interface as they are considered advanced.

### Fan Control configuration in `cgminer.conf`

There are three new configuration statements: `fan-mode` which selects which mode to use, `fan-temp` which sets temperature (in degree Celsius, and is valid only when `fan-mode: "temp"`), and `fan-speed` which sets speed of fan (as a percentage).

```
fan-mode: "temp"
fan-temp: "75"
```

```
fan-mode: "speed"
fan-speed: "90"
```

## AsicBoost support

Braiins OS supports overt (version-rolling) AsicBoost in accordance with [BIP310](https://github.com/bitcoin/bips/blob/master/bip-0310.mediawiki).

Trying to use AsicBoost on a pool that is not supporting it will result in an error message (the device will not start mining at all). Please note there is no automatic detection present at the moment, meaning AsicBoost can only be turned on/off manually.

### Antminer S9

AsicBoost is **turned on by default**. This setting can be changed in:

- web interface Services > CGMiner menu
- config file ```/etc/cgminer.conf``` by altering the ```multi-version``` value to `1` (disabled) or `4` (enabled)

### DragonMint T1

AsicBoost is **turned on by default** and **can not be turned off**. The device is incapable of mining efficiently without AsicBoost.

## Migrating from Braiins OS to factory firmware

You can restore the original/factory firmware using either your previously created backup or downloaded factory firmware image (only for Antminer S9!). In both cases, you have to download and unpack [transitional firmware image](#transitional-firmwares) and setup a virtual environment first.

### Setup virtual environment

#### On Linux

```bash
cd braiins-os_am1-s9_ssh_VERSION
virtualenv --python=/usr/bin/python3 .env
source .env/bin/activate
python3 -m pip install -r requirements.txt
```

#### On macOS

```bash
cd braiins-os_am1-s9_ssh_VERSION
virtualenv venv
source venv/bin/activate
pip install -r requirements.txt
```

#### On Windows

```bash
cd braiins-os_am1-s9_ssh_VERSION
mkvirtualenv bos
setprojectdir .
workon bos
pip install -r requirements.txt
```

### Flash the factory firmware

#### Using previously created backup

By default, a backup of the original firmware is created during the migration to Braiins OS.

Run (*on Windows, use `python` command instead of `python3`*):

```bash
python3 restore2factory.py backup/backup-id-date/ your-miner-hostname-or-ip
```

#### Using factory firmware image

On Antminer S9, you can alternatively flash a factory firmware image from the manufacturer's website, with `FACTORY_IMAGE` being file path or URL to the `tar.gz` (not extracted!) file. Supported images with corresponding MD5 hashes are listed in the [platform.py](https://github.com/braiins/braiins-os/blob/master/upgrade/am1/platform.py) file.

Run (*on Windows, use `python` command instead of `python3`*):

```bash
python3 restore2factory.py --factory-image FACTORY_IMAGE your-miner-hostname-or-ip
```

## Recovering Bricked (Unbootable) Devices Using SD Card

If anything goes wrong and your device seems unbootable, you can use the previously created SD card image to recover original firmware from the  manufacturer:

- Follow the steps in *Creating Bootable SD Card Image* to boot the device
- Run (*on Windows, use `python` command instead of `python3`*):
```
cd braiins-os_am1-s9_ssh_VERSION
python3 restore2factory.py backup/2ce9c4aab53c-2018-09-19/ your-miner-hostname-or-ip
```

After the script finishes, wait a few minutes and adjust the jumper to boot from NAND (internal memory) afterward.

## Firmware upgrade

Firmware upgrade process uses standard mechanism for installing/upgrading software packages within any OpenWrt based system. Follow the steps below to perform the firmware upgrade.

### Upgrade via web interface

The firmware periodically checks for availability of a new version. In case a new version is available, a blue **Upgrade** button appears on the right side of the top bar. Proceed to click on the button and confirm to start the upgrade.

Alternatively, you can update the repository information manually by clicking the *Update lists* button in the System > Software menu. In case the button is missing, try to refresh the page. To trigger the upgrade process, type `firmware` into the *Download and install package* field and press *OK*.

### Upgrade via SSH

```bash
# download latest packages from feeds server
$ opkg update
# try to upgrade to the latest firmware
$ opkg install firmware
```

Since the firmware installation results in a reboot, the following output is expected:

```
root@MINER:~# opkg install firmware
Upgrading firmware on root from 2018-09-22-0-853643de to 2018-09-22-1-8d9b127d...
Downloading https://feeds.braiins-os.org/am1-s9/firmware_2018-09-22-1-8d9b127d_arm_cortex-a9_neon.ipk
Running system upgrade...
--2018-09-22 14:23:47--  https://feeds.braiins-os.org/am1-s9/firmware_2018-09-22-1-8d9b127d_arm_cortex-a9_neon.tar
Resolving feeds.braiins-os.org... 104.25.97.101, 104.25.98.101, 2400:cb00:2048:1::6819:6165, ...
Connecting to feeds.braiins-os.org|104.25.97.101|:443... connected.
HTTP request sent, awaiting response... 200 OK
Length: 10373471 (9.9M) [application/octet-stream]
Saving to: '/tmp/sysupgrade.tar'

/tmp/sysupgrade.tar                     100%[==============================================================================>]   9.89M  10.7MB/s    in 0.9s

2018-09-22 14:23:48 (10.7 MB/s) - '/tmp/sysupgrade.tar' saved [10373471/10373471]

Collected errors:
* opkg_conf_load: Could not lock /var/lock/opkg.lock: Resource temporarily unavailable.
Saving config files...
Connection to 10.33.0.166 closed by remote host.
Connection to 10.33.0.166 closed.
```

### Upgrade from Braiins OS preview version

The script *bos2bos.py* (available in the repository, [clone it first](#cloning-the-braiins-os-repository)) is provided to upgrade from one of the Braiins OS preview versions and expects two parameters: URL to the transitional firmware (please refer to this [table](#transitional-firmwares)) and hostname or IP of the miner. You can also set up a new MAC address using the option `--mac`.

Follow the steps below to upgrade/transition from the Braiins OS preview.

```bash
python3 bos2bos.py transitional-firmware-url your-miner-hostname-or-ip
```

If you wish to upload the configuration using a yml file, use the `--config` option.

```bash
python3 bos2bos.py --config miner_cfg.yml transitional-firmware-url your-miner-hostname-or-ip
```

Here is a `miner_cfg.yml` config template you can modify according to your needs:

```
miner:
  # miner HW identifier
  hwid: Q50QDhdWuWq9yDr5
  # default miner pool
  pool:
    host: stratum+tcp://stratum.slushpool.com
    port: 3333
    user: '!non-existent-user!'
    pass: x
  # HW specific settings
  hw:
    freq: 550
    fixed_freq: true

net:
  # default miner MAC address
  mac: 00:0A:35:FF:FF:FF
  # static IP settings
  ip: 10.33.0.2
  mask: 255.255.255.0
  gateway: 10.33.0.1
  dns_servers:
  - 10.33.0.1
  # miner hostname when DHCP is used (static IP is not set)
  hostname: bos-miner
```

## Network miner discovery

### IP Report/Set button

Pressing IP Report (S9) or IP Set (T1) button sends a message `${IP},${MAC}` using UDP broadcast to a port 14235. When the IP Report button is pressed correctly, the **red LED** starts flashing for a while.

Alternatively, the IP Report can be enforced by calling ```bos ip_report``` from the command line.

The message and the protocol are compatible with the Bitmain IP Reporter. The default message and protocol can be changed in a UCI configuration file `/etc/config/bos` under 'ip_report' group.

### Discover using socat on linux

Listen for UDP broadcasts on you linux host using socat to get IP Report

```bash
socat - udp-recv:14235,broadcast
10.33.10.191,a0:b0:45:02:f5:35
```

### Discover script

The script *discover.py* (available in the repository, [clone it first](#cloning-the-braiins-os-repository)) is to be used to discover supported mining devices in the local network and has two working modes.

#### Listen mode

In this mode, IP and MAC addresses of the device are displayed after the IP Report button is pressed. Parameter `--format` can be used to change the default formatting of IP/MAC information.

```bash
python3 discover.py listen --format "{IP} ({MAC})"

10.33.10.191 (a0:b0:45:02:f5:35)
```

#### Scan mode

In this mode, the script scans the specified network range for supported devices. The parameter is expected to include a list of IP addresses or an IP subnetwork with a mask (example below) to scan a whole subnetwork.

For each device, the output includes a MAC address, IP address, system info, hostname, and a mining username configured.

```bash
python3 discover.py scan 10.55.0.0/24

50:6c:be:08:52:e5 (10.55.0.117) | bOS dm1-g19_2018-11-27-0-c34516b0 [nand] {511524 KiB RAM} dhcp(miner-w1) @userName.worker1
00:6c:aa:23:52:e1 (10.55.0.102) | DragonMint T1 G19 {250564 KiB RAM} dhcp(dragonMint) @userName.worker2
00:7e:92:77:a0:ca (10.55.0.133) | bOS am1-s9_2018-11-27-0-c34516b0 [nand] {1015120 KiB RAM} dhcp(miner-w3) @userName.worker3
00:94:cb:12:a0:ce (10.55.0.145) | Antminer S9 Fri Nov 17 17:57:49 CST 2017 (S9_V2.55) {1015424 KiB RAM} dhcp(antMiner) @userName.worker5
```

## Batch migration to Braiins OS

You can use simple bash scripts to install Braiins OS on a larger number of devices in sequence. For example, the following snippet will install a selected transitional image to all supported devices in the local network and print out their IP addresses.

```
IP_PREFIX=192.168.0; for i in `seq 1 150`; do ip=${IP_PREFIX}.$i; python3 ./upgrade2bos.py $ip || {echo Last IP processed: $ip}; done
```

For batch migration, you may also consider disabling backups with the `--no-nand-backup` option to speed up the process and save storage space. If you use this option, only config files will be backed up from each device.

## Setting miner password via SSH

You can set the miner's password via SSH from a remote host by running the below command and replacing *[newpassword]* with your own password.

*Note: bOS does **not** keep a history of the commands executed.*

```bash
$ ssh root@[miner-hostname-or-ip] 'echo -e "[newpassword]\n[newpassword]" | passwd'
```

To do this for several hosts in parallel you could use [p-ssh](https://linux.die.net/man/1/pssh).

## Reset to initial Braiins OS version

Uninstall the current firmware package to downgrade your firmware to the version which was initially installed when replacing the stock firmware.

```bash
$ opkg remove firmware
```

In addition to the above, reseting to initial Braiins OS version can also be initiated as follows:

* *IP SET button* - hold it for *10s* until red LED flashes
* *SD card* - first partition with FAT contains file *uEnv.txt* with a line **factory_reset=yes**
* *miner utility* - call ```miner factory_reset``` from the miner's command line

## Recovery Mode

Users don't typically have to enter recovery mode while using Braiins OS in a standard way. The ```restore2factory.py``` downgrade process uses it to restore the original factory firmware from the manufacturer. It can also be useful when repairing/investigating the currently installed system.

The recovery mode can be invoked in different ways:

* *IP SET button* - hold it for *3s* until green LED flashes
* *SD card* - first partition with FAT contains file *uEnv.txt* with a line **recovery=yes**
* *miner utility* - call ```miner run_recovery``` from the miner's command line

## Cloning the Braiins OS repository

Some Python tools/scripts are not included in the releases so you will need to clone the repository and set up the environment (if not already) to use them.

```bash
# clone repository
git clone https://github.com/braiins/braiins-os.git

cd braiins-os
virtualenv --python=/usr/bin/python3 .env
source .env/bin/activate
python3 -m pip install -r requirements.txt
```

*If you want to build Braiins OS yourself or modify the code, see the complete [developer guide](https://github.com/braiins/braiins-os).*
