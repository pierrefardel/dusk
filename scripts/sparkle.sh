#!/usr/bin/env bash
# Télécharge Sparkle (mises à jour automatiques) dans vendor/Sparkle, une seule fois.
# Version figée et somme de contrôle vérifiée.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=2.10.1
SHA256=1a856d2a9edb8e628de88121d7be04cf0e2167f8641650b78fca00b14244cf27
DEST=vendor/Sparkle

if [ -f "$DEST/.version" ] && [ "$(cat "$DEST/.version")" = "$VERSION" ]; then
  exit 0
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
ARCHIVE="$TMP/Sparkle.tar.xz"
curl -fsSL -o "$ARCHIVE" \
  "https://github.com/sparkle-project/Sparkle/releases/download/$VERSION/Sparkle-$VERSION.tar.xz"
echo "$SHA256  $ARCHIVE" | shasum -a 256 -c - >/dev/null

rm -rf "${DEST:?}"
mkdir -p "$DEST"
tar -xf "$ARCHIVE" -C "$DEST" Sparkle.framework bin
echo "$VERSION" > "$DEST/.version"
echo "Sparkle $VERSION : $DEST"
