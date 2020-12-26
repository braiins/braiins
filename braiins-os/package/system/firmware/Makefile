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

include $(TOPDIR)/rules.mk
include $(INCLUDE_DIR)/kernel.mk

PKG_NAME:=firmware
PKG_MAINTAINER:=Braiins <braiins@braiins.com>

# set firmware version
PKG_RELEASE:=$(call qstrip,$(CONFIG_FIRMWARE_VERSION))

BOS_FEEDS_CONFIG_RECORD:=$(call qstrip,$(CONFIG_FIRMWARE_FEEDS_RECORD))
BOS_FEEDS_CONFIG:=/etc/opkg/$(call qstrip,$(CONFIG_FIRMWARE_FEEDS_CONFIG)).conf
BOS_FEEDS_SERVER:=$(call qstrip,$(CONFIG_FIRMWARE_FEEDS_SERVER))/$(SUBTARGET)

# special custom configuration file which is kept during sysupgrade
BOS_CUSTOM_FEEDS_CONFIG:=/etc/opkg/$(call qstrip,$(CONFIG_FIRMWARE_CUSTOM_FEEDS_CONFIG)).conf
BOS_CUSTOM_FEEDS_SERVER:=$(call qstrip,$(CONFIG_FIRMWARE_CUSTOM_FEEDS_SERVER))/$(SUBTARGET)

include $(INCLUDE_DIR)/package.mk

define Package/firmware
  SECTION:=utils
  CATEGORY:=Utilities
  TITLE:=Current Firmware
endef

define Package/firmware/description
 Meta package for automated firmware upgrades.
endef

define Package/firmware/config
	source "$(SOURCE)/Config.in"
endef

define Package/firmware/conffiles
$(BOS_CUSTOM_FEEDS_CONFIG)
endef

define Build/Compile
endef

define Package/firmware/install
	$(INSTALL_DIR) $(1)/etc
	$(INSTALL_CONF) ./files/fw_info.json $(1)/etc/
	$(INSTALL_DIR) $(1)/etc/opkg
endef

define Package/firmware/preinst
#!/bin/sh

# if run within buildroot create default configs and exit
if [ -n "$${IPKG_INSTROOT}" ]; then
	echo "$(BOS_FEEDS_SERVER)" > $${IPKG_INSTROOT}$(BOS_FEEDS_CONFIG)
	cat > "$${IPKG_INSTROOT}$(BOS_CUSTOM_FEEDS_CONFIG)" <<-END
		# add your custom package feeds here
		#
		# $(BOS_CUSTOM_FEEDS_SERVER)
	END
	exit 0
fi

# ensure compatibility with old firmwares which does not have bos_mode marker
# moreover, old firmware has package installed only in NAND mode
if [ -f "/lib/functions/bos-defaults.sh" ]; then
	. /lib/functions/bos-defaults.sh

	# do not write to NAND when package installation has been forced for incorrect mode
	[ $$BOS_MODE == 'nand' ] || exit 1
fi

PACKAGES_NAME="Packages"
PACKAGES_PATH="/tmp/$${PACKAGES_NAME}"
SYSUPGRADE_PATH="/tmp/sysupgrade.tar"
BOS_FEEDS="$(BOS_FEEDS_CONFIG)"
BOS_FEEDS_RECORD="$(BOS_FEEDS_CONFIG_RECORD)"
FIRMWARE_NAME="$(PKG_NAME)_$(VERSION)_$(PKGARCH).tar"
FIRMWARE_REQUIRE="$(call qstrip,$(CONFIG_FIRMWARE_REQUIRE))"

if [ ! -f "$${BOS_FEEDS}" ]; then
	# fallback for old miner firmwares
	BOS_FEEDS="/etc/opkg/minerfeeds.conf"
	BOS_FEEDS_RECORD="miner_firmware"
fi
if [ ! -f "$${BOS_FEEDS}" ]; then
	echo "Missing configuration file $(BOS_FEEDS_CONFIG)!" >&2
	exit 1
fi

# parse server address from opkg configuration file
feeds_server=$$(awk '/^[^#]/ && $$2 ~ /^'$${BOS_FEEDS_RECORD}'$$/ {print $$3}' /etc/opkg/*.conf | tail -n1)

if [ -z "$${feeds_server}" ]; then
	echo "Cannot find feeds server in '/etc/opkg/' configuration files!" >&2
	echo "Configuration with '$${BOS_FEEDS_RECORD}' is probably missing." >&2
	exit 1
fi

current_fw_version() {
	if ! cat /etc/bos_version 2>/dev/null; then
		awk '/Package: /{p=$$2} /Version: /{v=$$2} \
			 /Status: /{if (p == "firmware" && $$NF == "installed") print v}' \
			'/usr/lib/opkg/status'
	fi
}

required_fw_version() {
	result=$$(awk '/Package: /{p=$$2} /Version: /{v=$$2} /Require: /{r=$$2} \
		/Filename: /{if (p == "firmware" && v == "'$$2'") print $$2" "r}' "$$1")
	eval $$3=$${result% *}
	eval $$4=$${result#* }
}

get_firmware_name() {
	local fw_version=$$(current_fw_version)
	local require=$${FIRMWARE_REQUIRE}
	local filename=$${FIRMWARE_NAME}
	if [ ! "$${fw_version}" "<" "$${require}" ]; then
		eval $$1=$${filename}
		return 0
	fi

	echo "Downloading package list..."
	wget -O "$${PACKAGES_PATH}.gz" "$${feeds_server}/$${PACKAGES_NAME}.gz" || return 1
	gzip -df "$${PACKAGES_PATH}.gz" || return 1
	wget -O "$${PACKAGES_PATH}.sig" "$${feeds_server}/$${PACKAGES_NAME}.sig" || return 1
	echo "Checking signature..."
	usign -V -m "$${PACKAGES_PATH}" -P "/etc/opkg/keys" || return 1

	while [ "$${fw_version}" "<" "$${require}" ]; do
		required_fw_version "$${PACKAGES_PATH}" $${require} filename require
		[ -n "$${filename}" ] || return 1
	done
	echo "Using previous firmware to satisfy requirements!"
	eval $$1="$${filename%.ipk}.tar"
}

if get_firmware_name firmware_name; then
	echo "Running system upgrade..."
	wget -O "$${SYSUPGRADE_PATH}" "$${feeds_server}/$${firmware_name}" || exit 1
	sysupgrade "$${SYSUPGRADE_PATH}"
else
	echo "Firmware version '$${FIRMWARE_REQUIRE}' is required before upgrading to this version." >&2
	echo "Cannot find required firmware on server!" >&2
	exit 1
fi

# the package is not actually installed
# wait to suppress error printing
sleep 10
exit 2
endef

define Package/firmware/prerm
#!/bin/sh

. /lib/functions/bos-defaults.sh

# check if package is being removed in NAND mode
if [ $$BOS_MODE != 'nand' ]; then
	echo "Firmware cannot be installed/removed in '$$BOS_MODE' mode!"
	echo "This functionality is available only for system running from NAND."
	if [ $$BOS_MODE == 'sd' ]; then
		echo
		echo "Please read documentation about installation of current system to NAND from SD card."
	fi
	echo
	exit 1
fi
endef

define Package/firmware/postrm
#!/bin/sh

. /lib/functions/bos-defaults.sh

# do not call factory reset when package removal has been forced for incorrect mode
[ $$BOS_MODE == 'nand' ] || exit 1

# if this script is run then simple package remove has been called
echo "Processing factory reset..."
bos factory_reset
endef

$(eval $(call BuildPackage,firmware))
