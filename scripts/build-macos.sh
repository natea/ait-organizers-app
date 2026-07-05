#!/usr/bin/env bash
# Build a signed (and, if credentials are present, notarized) macOS bundle.
#
# Signing identity is auto-detected from your login keychain, so you don't have
# to remember APPLE_SIGNING_IDENTITY. Notarization runs only if the notarization
# env vars are set (otherwise you get a signed-but-not-notarized build, which
# still fixes the keychain-prompt issue on your own Mac but needs the
# right-click→Open / `xattr -cr` step for other people).
#
# Usage:
#   bash scripts/build-macos.sh            # app + dmg
#   bash scripts/build-macos.sh --bundles app
#
# To notarize, set ONE of these credential sets in your shell first:
#   export APPLE_ID="you@example.com"
#   export APPLE_PASSWORD="app-specific-password"   # appleid.apple.com → App-Specific Passwords
#   export APPLE_TEAM_ID="YA3FM9C24T"
# ...or an App Store Connect API key:
#   export APPLE_API_KEY="KEYID"
#   export APPLE_API_ISSUER="ISSUER-UUID"
#   export APPLE_API_KEY_PATH="/path/to/AuthKey_KEYID.p8"
set -euo pipefail
cd "$(dirname "$0")/.."

# 1. Signing identity — auto-detect the Developer ID Application cert.
if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
  APPLE_SIGNING_IDENTITY="$(security find-identity -v -p codesigning \
    | awk -F'"' '/Developer ID Application/{print $2; exit}')"
fi
if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
  echo "❌ No 'Developer ID Application' certificate found in your login keychain."
  echo "   Create one: Xcode → Settings → Accounts → <team> → Manage Certificates → + → Developer ID Application"
  echo "   (requires paid Apple Developer Program membership)."
  exit 1
fi
export APPLE_SIGNING_IDENTITY
echo "🔏 Signing as: $APPLE_SIGNING_IDENTITY"

# 2. Notarization credentials?
if { [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; } \
   || { [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ] && [ -n "${APPLE_API_KEY_PATH:-}" ]; }; then
  echo "📤 Notarization credentials found — Tauri will sign, notarize, and staple."
else
  echo "⚠️  No notarization credentials set — build will be SIGNED but NOT notarized."
  echo "    Recipients would still need right-click→Open / 'xattr -cr'."
  echo "    Set APPLE_ID + APPLE_PASSWORD + APPLE_TEAM_ID (or the APPLE_API_* trio) to notarize."
fi

# 3. Build (passes through any extra args, e.g. --bundles app).
bun tauri build "$@"

# 4. Staple the DMG too. Tauri notarizes+staples the .app but leaves the .dmg
#    container unstapled, so a downloaded DMG needs an online Gatekeeper check.
#    Notarize the DMG itself and staple it so it verifies fully offline.
DMG="$(ls -t src-tauri/target/release/bundle/dmg/*.dmg 2>/dev/null | head -1 || true)"
if [ -n "${DMG:-}" ]; then
  if [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ] && [ -n "${APPLE_API_KEY_PATH:-}" ]; then
    echo "📤 Notarizing the DMG…"
    xcrun notarytool submit "$DMG" \
      --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER" --wait
    xcrun stapler staple "$DMG" && echo "📎 Stapled: $DMG"
  elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
    echo "📤 Notarizing the DMG…"
    xcrun notarytool submit "$DMG" \
      --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" --wait
    xcrun stapler staple "$DMG" && echo "📎 Stapled: $DMG"
  else
    echo "ℹ️  DMG left unstapled (no notarization credentials); the .app inside is still stapled."
  fi
fi

echo
echo "✅ Build complete. Artifacts under src-tauri/target/release/bundle/"
echo "   Verify signing/notarization:"
echo "     spctl -a -vvv -t install 'src-tauri/target/release/bundle/macos/AIT Mission Control.app'"
echo "     xcrun stapler validate 'src-tauri/target/release/bundle/dmg/'*.dmg"
echo "   (expected once notarized: 'accepted — source=Notarized Developer ID')"
