#!/bin/bash
# Quick test of statechart_backend with real EtherCAT

cd "$(dirname "$0")/../statechart_backend"

RUNTIME_BIN="../../target/release/trust-runtime"

echo "🔨 Building project..."
$RUNTIME_BIN build

echo ""
echo "🚀 Starting runtime with REAL HARDWARE..."
echo "   Control endpoint: unix:///tmp/trust-debug.sock"
echo "   EtherCAT: enp111s0 (EK1100 + EL2008)"
echo ""
echo "Press Ctrl+C to stop"
echo ""

# Run with sudo if needed for EtherCAT access
if [ "$EUID" -ne 0 ]; then
    echo "⚠️  EtherCAT may need sudo. If connection fails, run with:"
    echo "   sudo $0"
    echo ""
fi

$RUNTIME_BIN run --project .
