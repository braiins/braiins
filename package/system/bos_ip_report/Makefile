include $(TOPDIR)/rules.mk

PKG_NAME:=bos_ip_report
PKG_RELEASE:=1

PKG_MAINTAINER:=David Hauer <david.hauer@genesis-mining.com>

include $(INCLUDE_DIR)/package.mk


define Package/bos_ip_report
  SECTION:=utils
  CATEGORY:=Network
  TITLE:=IP-Address/Mac broadcaster via button press
  DEPENDS:=+socat
endef

define Package/bos_ip_report/description
 broadcast ip and mac address of device by pressing a button.
 Useful for finding the ip/mac of a specific miner in large installations.
endef

define Build/Compile
endef

define Package/bos_ip_report/install
	$(INSTALL_DIR) $(1)/usr/bin
	$(INSTALL_BIN) ./files/ip-report.sh $(1)/usr/bin/ip-report.sh

	$(INSTALL_DIR) $(1)/etc/hotplug.d/button/
	$(INSTALL_BIN) ./files/00-button $(1)/etc/hotplug.d/button/00-button
endef

$(eval $(call BuildPackage,bos_ip_report))
