#!/bin/sh
set -e

if [ "$1" = "configure" ]; then
    systemctl daemon-reload || true
    if ! systemctl is-enabled ups-exporter >/dev/null 2>&1; then
        systemctl enable ups-exporter || true
    fi
    systemctl restart ups-exporter || true
fi
