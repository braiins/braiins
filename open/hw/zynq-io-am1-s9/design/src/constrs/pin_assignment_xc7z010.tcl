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
# connectors J1..J9 - PLUG0
set_property -dict { PACKAGE_PIN T11 IOSTANDARD LVCMOS33 PULLDOWN true } [get_ports { gpio_0_tri_i[0]  }];  # S9: J1_5, PLUG0
set_property -dict { PACKAGE_PIN R19 IOSTANDARD LVCMOS33 PULLDOWN true } [get_ports { gpio_0_tri_i[1]  }];  # S9: J2_5, PLUG0
set_property -dict { PACKAGE_PIN T14 IOSTANDARD LVCMOS33 PULLDOWN true } [get_ports { gpio_0_tri_i[2]  }];  # S9: J3_5, PLUG0
set_property -dict { PACKAGE_PIN Y16 IOSTANDARD LVCMOS33 PULLDOWN true } [get_ports { gpio_0_tri_i[3]  }];  # S9: J4_5, PLUG0
set_property -dict { PACKAGE_PIN T16 IOSTANDARD LVCMOS33 PULLDOWN true } [get_ports { gpio_0_tri_i[4]  }];  # S9: J5_5, PLUG0
set_property -dict { PACKAGE_PIN U14 IOSTANDARD LVCMOS33 PULLDOWN true } [get_ports { gpio_0_tri_i[5]  }];  # S9: J6_5, PLUG0
set_property -dict { PACKAGE_PIN T20 IOSTANDARD LVCMOS33 PULLDOWN true } [get_ports { gpio_0_tri_i[6]  }];  # S9: J7_5, PLUG0
set_property -dict { PACKAGE_PIN Y18 IOSTANDARD LVCMOS33 PULLDOWN true } [get_ports { gpio_0_tri_i[7]  }];  # S9: J8_5, PLUG0
set_property -dict { PACKAGE_PIN R16 IOSTANDARD LVCMOS33 PULLDOWN true } [get_ports { gpio_0_tri_i[8]  }];  # S9: J9_5, PLUG0


####################################################################################################
# Pin assignment for GPIO 1 - outputs
####################################################################################################
# LEDs
set_property -dict { PACKAGE_PIN M19 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[0]  }];  # S9: D5, LED
set_property -dict { PACKAGE_PIN M17 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[1]  }];  # S9: D6, LED
set_property -dict { PACKAGE_PIN F16 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[2]  }];  # S9: D7, LED
set_property -dict { PACKAGE_PIN L19 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[3]  }];  # S9: D8, LED

# connectors J1..J9 - RST
set_property -dict { PACKAGE_PIN T10 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[4]  }];  # S9: J1_15, RST
set_property -dict { PACKAGE_PIN V13 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[5]  }];  # S9: J2_15, RST
set_property -dict { PACKAGE_PIN T15 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[6]  }];  # S9: J3_15, RST
set_property -dict { PACKAGE_PIN Y17 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[7]  }];  # S9: J4_15, RST
set_property -dict { PACKAGE_PIN U17 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[8]  }];  # S9: J5_15, RST
set_property -dict { PACKAGE_PIN U15 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[9]  }];  # S9: J6_15, RST
set_property -dict { PACKAGE_PIN U20 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[10] }];  # S9: J7_15, RST
set_property -dict { PACKAGE_PIN Y19 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[11] }];  # S9: J8_15, RST
set_property -dict { PACKAGE_PIN R17 IOSTANDARD LVCMOS33 } [get_ports { gpio_1_tri_o[12] }];  # S9: J9_15, RST


####################################################################################################
# Pin assignment for fans
####################################################################################################
set_property -dict {PACKAGE_PIN J18  IOSTANDARD LVCMOS33  DRIVE 16} [get_ports {pwm}];  # {FAN1 .. FAN6}.PWM

set_property -dict {PACKAGE_PIN G17  IOSTANDARD LVCMOS33  PULLUP true} [get_ports {fan_sense[0]}];  # FAN1.SENSE
set_property -dict {PACKAGE_PIN H20  IOSTANDARD LVCMOS33  PULLUP true} [get_ports {fan_sense[1]}];  # FAN2.SENSE

####################################################################################################
# Pin assignment for I2C
####################################################################################################
set_property -dict { PACKAGE_PIN W18 IOSTANDARD LVCMOS33 PULLUP true DRIVE 16} [get_ports { IIC_HB_scl_io }];  # S9: Jx_4, TSCL
set_property -dict { PACKAGE_PIN W19 IOSTANDARD LVCMOS33 PULLUP true DRIVE 16} [get_ports { IIC_HB_sda_io }];  # S9: Jx_3, TSDA


####################################################################################################
# Pin assignment for UARTs
####################################################################################################
# connectors J1..J9 - RXD
# set_property -dict { PACKAGE_PIN U12 IOSTANDARD LVCMOS33 PULLUP true } [get_ports { rxd_0 }];  # S9: J1_12, RX
# set_property -dict { PACKAGE_PIN W13 IOSTANDARD LVCMOS33 PULLUP true } [get_ports { rxd_1 }];  # S9: J2_12, RX
# set_property -dict { PACKAGE_PIN R14 IOSTANDARD LVCMOS33 PULLUP true } [get_ports { rxd_2 }];  # S9: J3_12, RX
# set_property -dict { PACKAGE_PIN Y14 IOSTANDARD LVCMOS33 PULLUP true } [get_ports { rxd_3 }];  # S9: J4_12, RX
# set_property -dict { PACKAGE_PIN W15 IOSTANDARD LVCMOS33 PULLUP true } [get_ports { rxd_4 }];  # S9: J5_12, RX
set_property -dict { PACKAGE_PIN U19 IOSTANDARD LVCMOS33 PULLUP true } [get_ports { rxd_5 }];  # S9: J6_12, RX
set_property -dict { PACKAGE_PIN W20 IOSTANDARD LVCMOS33 PULLUP true } [get_ports { rxd_6 }];  # S9: J7_12, RX
set_property -dict { PACKAGE_PIN W16 IOSTANDARD LVCMOS33 PULLUP true } [get_ports { rxd_7 }];  # S9: J8_12, RX
# set_property -dict { PACKAGE_PIN R18 IOSTANDARD LVCMOS33 PULLUP true } [get_ports { rxd_8 }];  # S9: J9_12, RX

# connectors J1..J9 - TXD
# set_property -dict { PACKAGE_PIN T12 IOSTANDARD LVCMOS33 } [get_ports { txd_0 }];  # S9: J1_11, TX
# set_property -dict { PACKAGE_PIN V12 IOSTANDARD LVCMOS33 } [get_ports { txd_1 }];  # S9: J2_11, TX
# set_property -dict { PACKAGE_PIN P14 IOSTANDARD LVCMOS33 } [get_ports { txd_2 }];  # S9: J3_11, TX
# set_property -dict { PACKAGE_PIN W14 IOSTANDARD LVCMOS33 } [get_ports { txd_3 }];  # S9: J4_11, TX
# set_property -dict { PACKAGE_PIN V15 IOSTANDARD LVCMOS33 } [get_ports { txd_4 }];  # S9: J5_11, TX
set_property -dict { PACKAGE_PIN U18 IOSTANDARD LVCMOS33 } [get_ports { txd_5 }];  # S9: J6_11, TX
set_property -dict { PACKAGE_PIN V20 IOSTANDARD LVCMOS33 } [get_ports { txd_6 }];  # S9: J7_11, TX
set_property -dict { PACKAGE_PIN V16 IOSTANDARD LVCMOS33 } [get_ports { txd_7 }];  # S9: J8_11, TX
# set_property -dict { PACKAGE_PIN T17 IOSTANDARD LVCMOS33 } [get_ports { txd_8 }];  # S9: J9_11, TX

