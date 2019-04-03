
  #!/bin/sh
  IP=$(ip route get 1.2.3.4 | awk '{print $7}')
  MAC=$(cat /sys/class/net/eth0/address)

  echo -n "$IP,$MAC" | socat -u STDIO UDP-DATAGRAM:255.255.255.255:14235,broadcast
