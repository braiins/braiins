####################################################################################################
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
####################################################################################################

####################################################################################################
# Pin assignment for GPIO 0 - inputs
####################################################################################################
# connectors J1..J4 - PLUG
set_property -dict {PACKAGE_PIN H13  IOSTANDARD LVCMOS33  PULLDOWN true} [get_ports {gpio_0_tri_i[0]}];  # J1.PLUG
set_property -dict {PACKAGE_PIN J14  IOSTANDARD LVCMOS33  PULLDOWN true} [get_ports {gpio_0_tri_i[1]}];  # J2.PLUG
set_property -dict {PACKAGE_PIN M15  IOSTANDARD LVCMOS33  PULLDOWN true} [get_ports {gpio_0_tri_i[2]}];  # J3.PLUG
set_property -dict {PACKAGE_PIN L13  IOSTANDARD LVCMOS33  PULLDOWN true} [get_ports {gpio_0_tri_i[3]}];  # J4.PLUG


####################################################################################################
# Pin assignment for GPIO 1 - outputs
####################################################################################################
# connectors J1..J9 - RST
set_property -dict {PACKAGE_PIN G12  IOSTANDARD LVCMOS33} [get_ports {gpio_1_tri_o[0]}];  # J1.RST
set_property -dict {PACKAGE_PIN J13  IOSTANDARD LVCMOS33} [get_ports {gpio_1_tri_o[1]}];  # J2.RST
set_property -dict {PACKAGE_PIN L15  IOSTANDARD LVCMOS33} [get_ports {gpio_1_tri_o[2]}];  # J3.RST
set_property -dict {PACKAGE_PIN K13  IOSTANDARD LVCMOS33} [get_ports {gpio_1_tri_o[3]}];  # J4.RST

# LEDs
set_property -dict {PACKAGE_PIN J11  IOSTANDARD LVCMOS33} [get_ports {gpio_1_tri_o[4]}];  # LED D3


####################################################################################################
# Pin assignment for fans
####################################################################################################
set_property -dict {PACKAGE_PIN E11  IOSTANDARD LVCMOS33  DRIVE 16} [get_ports {pwm}];  # FANx.PWM

set_property -dict {PACKAGE_PIN F12  IOSTANDARD LVCMOS33  PULLUP true} [get_ports {fan_sense[0]}];  # FAN1.SENSE
set_property -dict {PACKAGE_PIN E13  IOSTANDARD LVCMOS33  PULLUP true} [get_ports {fan_sense[1]}];  # FAN2.SENSE


####################################################################################################
# Pin assignment for I2C
####################################################################################################
set_property -dict {PACKAGE_PIN K11  IOSTANDARD LVCMOS33  PULLUP true DRIVE 16} [get_ports {IIC_HB_scl_io}];  # Jx.TSCL
set_property -dict {PACKAGE_PIN K12  IOSTANDARD LVCMOS33  PULLUP true DRIVE 16} [get_ports {IIC_HB_sda_io}];  # Jx.TSDA


####################################################################################################
# Pin assignment for UARTs
####################################################################################################
# connectors J1..J4 - RXD
set_property -dict {PACKAGE_PIN G11  IOSTANDARD LVCMOS33  PULLUP true} [get_ports {rxd_0}];  # J1.Rx
set_property -dict {PACKAGE_PIN J15  IOSTANDARD LVCMOS33  PULLUP true} [get_ports {rxd_1}];  # J2.Rx
set_property -dict {PACKAGE_PIN N13  IOSTANDARD LVCMOS33  PULLUP true} [get_ports {rxd_2}];  # J3.Rx
set_property -dict {PACKAGE_PIN L14  IOSTANDARD LVCMOS33  PULLUP true} [get_ports {io_rxd_3}];  # J4.Rx, FAN3.SENSE

# connectors J1..J4 - TXD
set_property -dict {PACKAGE_PIN H12  IOSTANDARD LVCMOS33} [get_ports {txd_0}];  # J1.Tx
set_property -dict {PACKAGE_PIN K15  IOSTANDARD LVCMOS33} [get_ports {txd_1}];  # J2.Tx
set_property -dict {PACKAGE_PIN N14  IOSTANDARD LVCMOS33} [get_ports {txd_2}];  # J3.Tx
set_property -dict {PACKAGE_PIN M14  IOSTANDARD LVCMOS33} [get_ports {io_txd_3}];  # J4.Tx, FAN4.SENSE


####################################################################################################
# Pin assignment for PSU interface
####################################################################################################
set_property -dict {PACKAGE_PIN F15  IOSTANDARD LVCMOS33  PULLUP true DRIVE 16} [get_ports {IIC_PSU_scl_io}];  # SCL
set_property -dict {PACKAGE_PIN H14  IOSTANDARD LVCMOS33  PULLUP true DRIVE 16} [get_ports {IIC_PSU_sda_io}];  # SDA


####################################################################################################
# Ethernet MII & MDIO interface
####################################################################################################
set_property -dict {PACKAGE_PIN L12  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_tx_clk} ];   # MII.TXCLK, 25 MHz clock
set_property -dict {PACKAGE_PIN M12  IOSTANDARD LVCMOS33} [ get_ports {MDIO_ETH_0_mdio_io} ];  # MDIO.MDIO, pull-up 1k5
set_property -dict {PACKAGE_PIN N11  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_rx_clk} ];   # MII.RXCLK, 25 MHz clock
set_property -dict {PACKAGE_PIN N12  IOSTANDARD LVCMOS33} [ get_ports {MDIO_ETH_0_mdc} ];      # MDIO.MDC, pull-up 1k5
set_property -dict {PACKAGE_PIN P15  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_rxd[0]} ];   # MII.RXD0
set_property -dict {PACKAGE_PIN R15  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_rxd[1]} ];   # MII.RXD1
set_property -dict {PACKAGE_PIN P11  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_rxd[2]} ];   # MII.RXD2
set_property -dict {PACKAGE_PIN R11  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_rxd[3]} ];   # MII.RXD3
set_property -dict {PACKAGE_PIN R12  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_rx_dv} ];    # MII.RXDV
set_property -dict {PACKAGE_PIN R13  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_rx_er} ];    # MII.RXER, MII.RXD4
set_property -dict {PACKAGE_PIN P13  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_crs} ];      # MII.CRS
set_property -dict {PACKAGE_PIN P14  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_col} ];      # MII.COL, MII.CRS_DV
set_property -dict {PACKAGE_PIN M9   IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_txd[0]} ];   # MII.TXD0
set_property -dict {PACKAGE_PIN N9   IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_txd[1]} ];   # MII.TXD1
set_property -dict {PACKAGE_PIN R7   IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_txd[2]} ];   # MII.TXD2
set_property -dict {PACKAGE_PIN R8   IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_txd[3]} ];   # MII.TXD3
set_property -dict {PACKAGE_PIN M10  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_tx_en[0]} ]; # MII.TXEN
set_property -dict {PACKAGE_PIN M11  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_tx_er[0]} ]; # MII.TXER, MII.TXD4

# not used data inputs & outputs
set_property -dict {PACKAGE_PIN N7   IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_txd[4]} ];   #
set_property -dict {PACKAGE_PIN N8   IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_txd[5]} ];   #
set_property -dict {PACKAGE_PIN P8   IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_txd[6]} ];   #
set_property -dict {PACKAGE_PIN P9   IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_txd[7]} ];   #
set_property -dict {PACKAGE_PIN E12  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_rxd[4]} ];   # log. 0
set_property -dict {PACKAGE_PIN F13  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_rxd[5]} ];   # log. 1
set_property -dict {PACKAGE_PIN F14  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_rxd[6]} ];   # log. 0
set_property -dict {PACKAGE_PIN G15  IOSTANDARD LVCMOS33} [ get_ports {GMII_ETH_0_rxd[7]} ];   # log. 1

