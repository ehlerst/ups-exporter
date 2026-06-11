#!/bin/sh
set -e

if [ "$1" = "remove" ]; then
    systemctl stop ups-exporter || true
    systemctl disable ups-exporter || true
fi
