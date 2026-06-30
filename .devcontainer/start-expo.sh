#!/usr/bin/env bash
#
# Auto-started by the devcontainer (postStartCommand) so a GitHub Codespace
# serves the Expo dev server with ZERO terminal input. Open the printed
# exp:// URL in Expo Go (or, if EXPO_TOKEN is set as a Codespaces secret,
# the dev server appears automatically in Expo Go under your account).
#
# Codespaces exposes Metro publicly at ${CODESPACE_NAME}-8081.app.github.dev,
# so no ngrok/tunnel and no QR are needed.

set -u

APP_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../apps/mobile/ios" && pwd)"
cd "$APP_DIR" || exit 1

HOST="${CODESPACE_NAME:-localhost}-8081.app.github.dev"
echo "REACT_NATIVE_PACKAGER_HOSTNAME=${HOST}" > .env.local

URL="exp://${HOST}"
BANNER="/workspaces/OPEN-IN-EXPO-GO.txt"
{
  echo "========================================"
  echo "  Open this in Expo Go (tap or paste):"
  echo "  ${URL}"
  echo "========================================"
} | tee "${BANNER}" 2>/dev/null || echo "${URL}"

# Restart cleanly if a previous server is still around.
pkill -f "expo start" 2>/dev/null || true

# Keep Metro running in the background so the Codespace "start" step finishes;
# logs go to /tmp/expo.log.
nohup npx expo start --port 8081 > /tmp/expo.log 2>&1 &

echo "Expo dev server starting in the background (logs: /tmp/expo.log)."
