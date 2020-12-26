# 2020-04-30-2

## Overview

This is a quick bugfix release that address problem with building the firmware locally.

## All mining hardware types

- [bug] build script has been simplified and produces SD card images directly from inside the docker run instead of generating bash scripts


# 2020-04-30-0-259943b5

## Overview

This release covers mostly user facing issues, installation/uninstallation pain points, and 1 major problem with I2C controller on S9s. Also, we now have nightly builds that are easy to enable via the **bos** tool.

## All mining hardware types

- [feature] support for reconnect - we have implemented support for `client.reconnect` (stratum V1) and reconnect message for V2
- [feature] installation/uninstallation process (aka **upgrade2bos** for transitioning from factory firmware to Braiins OS and **restore2factory** for reverting back to factory firmware) has been improved:
  - [feature] custom pool user (`--pool-user`) can be set on command line
  - [feature] pool settings from the factory firmware are now automatically being migrated to the BOSminer configuration. Migration can be disabled by specifying (`--no-keep-pools`)
  - [feature] time and disk space consuming backup of the original firmware is now disabled by default (can be enabled by `--backup`)
  - [feature] keeping the host name while performing a first-time install is now driven by 2 options `--keep-hostname` and `--no-keep-hostname` allowing users to force override and automatic hostname generation based on MAC address
- [feature] support for enabling/disabling nightly builds has been integrated into **bos** utility (and its legacy **miner** counterpart).
- [feature] system now provides **logs** covering **longer timespan** of **BOSminer** operation due to enabling **log rotation** and compression of '/var/log/syslog.old' when it is bigger than 32 kB
- [bug] SD card image now contains the Slush Pool authority public key that was missing
- [bug] rejection rate is now being displayed correctly 
- [bug] unknown stratum V1 messages received from the server are now being logged for diagnostics

## Antminer S9

- [bug] some devices were experiencing random I2C controller bus lockups and would fail to communicate with hashboard power controllers connected to the shared I2C bus. We have found out that the cause was the Xilinx I2C controller core that we have integrated into the FPGA bitstream. We have switched to the I2C present in the SoC and the bitstream only routes the signal of the peripheral (IIC0) to corresponding FPGA pins.

# 2020-03-29-0

## Overview

## All mining hardware types


Dear **CGminer**, thank you for all the proof of works you've delivered and for the many challenges provided after your source code was forked and closed by hardware manufacturers. Special thanks to **Con Kolivas** - the original author of CGminer - for all the years of hard work on creating and maintaining this fundamental component of Bitcoin ecosystem, even as it became an impossible task. This release marks a significant step forward in **Braiins OS** development, since it replaces **CGminer** with a new software called **BOSminer** written from scratch in Rust. CGminer has been with us since the GPU days and helped Bitcoin grow to what it is today. RIP - Rust In Peace, CGminer.
The goal of the release was to replicate the existing feature set of the **CGminer**-based **Braiins OS**, while using the new mining software.


## All mining hardware types

- [feature] CGminer has been replaced by *BOSminer*. [README](../open/bosminer/README.md) provides additional details about features and known issues
- [feature] *secure* connection support for Stratum V2 based on a Noise protocol framework. How it works: your pool operator provides you with their public key. This allows the software to verify the identity of the upstream mining endpoint. The certificate of the upstream mining endpoint should have a limited lifetime. The security on the connection is mandatory under the standard URL prefix.
- [feature] Stratum V2 URL doesn't require specifying ports. Just fill-in: `stratum2+tcp://v2.stratum.slushpool.com/u95GEReVMjK6k5YqiSFNqqTnKU4ypU2Wm8awa6tmbmDmk1bWt` and you are ready to go. The default port is 3336 or 3337 depending on the protocol release cycle. Motivation: we wanted to push the new protocol as much as possible since it represents a significant quality improvement (see https://stratumprotocol.org/ for details). At the same time, some changes may happen since it is still in draft form. Therefore, with every protocol breaking release, we would switch to the other port from the pair. Right now, we are starting with port number **3336**. This approach means that users don't have to switch ports manually and edit their configuration after upgrading to a new release that may also contain a protocol upgrade. Protocol upgrades will always be listed in this document.
- [feature] *web interface* has been completely reworked including statistics, configuration editor, etc.

## Antminer S9

- [feature] currently, BOSminer supports only standard 3 hashboard builds (stock S9, S9i, S9j), meaning it is not currently suitable for custom-built hardware with more than 3 hashboards.

## Dragonmint T1

- [feature] support for this hardware has been dropped for the time being.

# 2019-06-05-0

## All mining hardware types

- [feature] YES, we have a **new logo** that is consistent with the new Braiins logo, hope you like it ;-)
- [feature] the **IP report button** feature now broadcasts a UDP datagram compatible with the original S9 factory firmware
- [feature] the miner now stores the reason of the **last cgminer exit/failure** in ```/tmp/cgminer_quit_reason```. The miner status page now presents the actual reason along with a confirmation button to clear the last failure status
- [feature] fix **web status** data loading when connecting to miners in remote locations with considerable latencies
- [feature] **universal SD card images** - each SD card image tries to detect the device MAC address from any firmware image that is present in NAND flash. This simplifies using the same SD card image on multiple devices without the need for manually generating editing ```uEnv.txt``` on each card
- [feature] it is now possible to install Braiins OS into flash memory from a running **SD card** - see the docs for details
- [feature] increase stratum connect() timeout to 3 seconds to prevent harassing stratum servers too early when probing all the IP addresses for a given URL
- [feature] attempting to perform ```opkg install firmware``` on a Braiins OS instance **running from SD card** now visibly tells the user that this is not a supported operation (i.e. you should obtain a new SD card image and flash it to your SD card)
- [feature] **TUN/TAP module** is now part of the base system as we cannot use upstream modules due to custom kernel. These can be useful for OpenVPN
- [bug] ```#xnsub``` support for NH stratum extension has been fixed
- [bug] a few **memory leaks** in bmminer and CGminer API have been fixed

## Antminer S9

- [feature] there are 2 new options:
   ```--disable-sensors``` - the mining software acts as if there were no temperature sensors found, meaning that the fan would be running at 100% unless set to some manually override value.
   ```--disable-remote-sensors``` - the mining software only takes temperature measurements from the hash boards and doesn't try to measure temperatures on the mining chips. The actual mining chip temperature is being estimated based on the hash board temperature plus some empirically set offset (15 C)
- [bug] fix support for non-asicboost pools by eliminating **mining.configure** in case the number of configured midstates is 1 (AsicBoost disabled)

# 2019-02-21-0

## All mining hardware types

- [feature] **temperature limits** are now configurable via the configuration file. The two new configuration options are: ```--fan-hot-temp``` and ```--fan-dangerous-temp```. These options effectively override the temperature limits in ```temp-def.h```
- [fix] **15m** and **24h hash rate** show up only if the **full time period** of hash rate data has been collected
- [fix] enabled/disabled **indicator** for **Asic Boost** now works again in the overview status page
- [feature] **fan control** now has a new option ```--min-fans``` that specifies the minimum amount of fans that have to be operational (defaults to 1). Setting the option to "0" results in skipping the check for working fans.

## Antminer S9

- [feature] new configuration option allows **disabling temperature sensor scanning** (```--no-sensor-scan```). I2C scan log is now being stored into a separate file: ```/tmp/i2c_scan.log```
- [feature] support for **ADT7461 temperature sensor** that appears to be used on some hash board revisions

## Dragonmint T1

- [feature] support for **G29 revision** of the control board (no SD-card slot)
- [feature] web interface now allows configuring **full range of frequencies** (120 MHz - 1596 MHz) and **voltage levels** (1-31)
- [feature] transitional firmware for Dragonmints T1 with **G19** control board is no longer provided. See the documentation for details.

# 2019-01-24-0

## All mining hardware types

- [feature] bOS now automatically **detects availability** of a new version. The web UI now contains an indicator of new release availability (+ single click to install)
- [feature] **firmware upgrade process** is now more smooth when upgrading from **bOS** that is more than **2 releases old**
- [feature] miner status web page **no longer needs access to port 4028** of the miner, everything is provided via web proxy on the miner
- [feature] a new script **discover.py** scans the network range and provides information about **bOS devices** as well as **factory firmware devices**
- [feature] **fancontrol** completely rewritten, all mining hardware now uses the same **PID** controller algorithm. The automated fan control can be overriden and fan speed can be set manually
- [feature] it is now possible to run **upgrade2bos.py** with **--dry-run** parameter to create system backup and check if the firmware is likely succeed in transitioning to bOS
- [feature] **miner status page** is now the **default** section after login
- [feature] transition from factory firmware to bOS can now be supplied with a **post-upgrade script** that runs during the **first boot** of the machine running bOS for the first time. Official documentation provides more details.
- [feature] **macOS guide** for factory firmware transition added
- [feature] DHCP client now sends its **system hostname** to its DHCP server = there is a single source of truth with regards to the machine hostname

## Antminer S9

- [feature] upgrade to bOS is now possible for S9's running older firmware that has **4 NAND partitions**
- [feature] a multiplier allows changing **frequency** of either **per-chip calibration settings from the factory** or of user configured **per hash board base frequency**. Web interace adjusted accordingly. The functionality is also available through the API.
- [feature] it is now possible to restore the factory firmware **without having a backup** of the original firmware. The configuration is tailored from the running bOS and the restore2factory.py tool can be supplied with a factory firmware image downloaded from manufacturer's website.
- [feature] firmware now supports the **reset button** used for rebooting the machine. If the push button is held down for more than 5 seconds the machine is also "factory" reset and all bOS settings are erased (Note, that it doesn't switch back to original factory firmware)


# 2018-11-27-0

## Overview - major release; code name: Cobalt

A new major release that brings important features for S9's and unifies miner status page for all supported devices.

## Antminer S9

- [feature] per chip frequency tuning based on factory calibration constants
- [feature] alternative transitional firmware (```braiins-os_am1-s9_web_*tar.gz```) image that can be flashed via **factory
  web interface**
- [feature] support for S9j and R4 hardware types

## All mining hardware types

- [feature] new **miner overview** page with real-time telemetry data
- [feature] hash rate averaging exponential decay function in cgminer replaced with windowed average
- [feature] login text now indicates when running in recovery mode
- [feature] factory transition firmware preserves network settings of the original firmware. Optionally, user may request keeping the hostname, too (`upgrade2bos.py --keep-hostname`).
- [feature] bOS mode and version are now stored in `/etc/bos_{mode,version}`
- [feature] *upgrade2bos.py* can now skip NAND backup entirely and provide *configuration backup only*. The reason is to save space and speed up the process of upgrading big farms in bulk. The reverse script *restore2factory.py* can take the original factory image and combine it with the saved configuration. Thus, eliminating the need for full NAND backups completely.
- [feature] restore2factory now automatically detects when being run from SD card and acts accordingly
- [feature] new LED signalization scheme - see user documentation for details

### cgminer API improvements

- [feature] support for HTTP GET requests
- [feature] calculation of hourly hardware error rate
- [feature] Asic Boost status (yes/no)
- [feature] hashrate statistics for 1m, 15m, 24h using windowed average


# 2018-10-24-0

## Overview

*This release only contains images for Antminer S9.*

**Important:** If you wish to upgrade firmware (package `firmware`) via the web interface, it is neccesary to install package `libustream-openssl` first. This step is not required when upgrading via SSH.

## Antminer S9

- [feature] bmminer now supports overt **AsicBoost** via version rolling, latest bitstream from Bitmain
  has been integrated and [BIP310](https://github.com/bitcoin/bips/blob/master/bip-0310.mediawiki) support has been enabled. AsicBoost can be turned off in the interface.
- [feature] the transitional firmware now supports **flashing Antminers S9** in addition to originally supported S9i
- [feature] **per chain** frequency and **voltage control** now available in the interface
- [fix] Temperature reporting has been corrected to take measurements from
  the 'middle' sensor that is placed in the hot area of each
  hashboard. The displayed temperatures should better reflect the true
  temperature of the hashboard.

## All hardware types

- [fix] package list update triggered via web UI doesn't report missing SSL support error anymore
- [fix] opkg no longer reports errors about missing feeds due to an attempt to fetch
- [fix] Firmware reports its real release version during stratum subscribe. The default cgminer version has been removed.
