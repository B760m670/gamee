#!/bin/bash
HOST="${CODESPACE_NAME}-8081.app.github.dev"
echo "REACT_NATIVE_PACKAGER_HOSTNAME=${HOST}" > .env.local
echo ""
echo "========================================"
echo "  Expo Go URL:"
echo "  exp://${HOST}"
echo "========================================"
echo ""
fuser -k 8081/tcp 2>/dev/null || true
npx expo start --port 8081
