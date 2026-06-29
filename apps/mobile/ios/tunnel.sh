#!/bin/bash

METRO_PORT=8081
PROXY_PORT=8082

if [ ! -f ./bore ]; then
    echo "Downloading bore..."
    curl -sL https://github.com/ekzhang/bore/releases/download/v0.5.1/bore-v0.5.1-x86_64-unknown-linux-musl.tar.gz | tar xz
fi

fuser -k ${METRO_PORT}/tcp 2>/dev/null || true
fuser -k ${PROXY_PORT}/tcp 2>/dev/null || true
pkill bore 2>/dev/null || true
rm -f /tmp/bore.log

export REACT_NATIVE_PACKAGER_HOSTNAME=bore.pub
echo "REACT_NATIVE_PACKAGER_HOSTNAME=bore.pub" > .env.local

# Start proxy first
PROXY_PORT=$PROXY_PORT METRO_PORT=$METRO_PORT node proxy.js &
sleep 1

# Start bore — auto-assigns a port, tunnels to proxy
./bore local $PROXY_PORT --to bore.pub > /tmp/bore.log 2>&1 &

# Wait for bore to connect and get port
echo "Getting tunnel port..."
for i in $(seq 1 40); do
    BORE_PORT=$(grep -oE 'bore\.pub:[0-9]+' /tmp/bore.log 2>/dev/null | tail -1 | grep -oE '[0-9]+$')
    [ -n "$BORE_PORT" ] && break
    sleep 1
done

if [ -z "$BORE_PORT" ]; then
    echo "ERROR: bore failed"
    cat /tmp/bore.log
    exit 1
fi

echo ""
echo "========================================"
echo "  exp://bore.pub:${BORE_PORT}"
echo "========================================"
echo ""

npx expo start --port $METRO_PORT
