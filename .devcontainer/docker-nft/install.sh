#!/bin/sh
set -eu
# The host uses nftables; the Debian Docker feature otherwise selects legacy.
update-alternatives --set iptables /usr/sbin/iptables-nft
update-alternatives --set ip6tables /usr/sbin/ip6tables-nft
