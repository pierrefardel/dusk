#!/usr/bin/env bash
# Prépare une version de Dusk : numéro, build signé, zip, signature Sparkle,
# entrée dans appcast.xml et brouillon de version sur le dépôt public.
# Ne publie rien et ne pousse rien.
#
#   ./scripts/release.sh 0.1.1
#
# Ensuite, dans l'ordre (le flux ne doit pas annoncer un zip pas encore en ligne) :
#   1. publier le brouillon sur GitHub ;
#   2. commit privé, ./scripts/publish.sh, push des deux dépôts (appcast.xml part avec).
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="${1:?usage : ./scripts/release.sh X.Y.Z}"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Numéro invalide : $VERSION" >&2; exit 1; }
REPO=pierrefardel/dusk
TAG="v$VERSION"
ZIP=dist/Dusk.zip

# Numéro de version (Cargo.toml du workspace), repris par bundle.sh dans l'Info.plist.
sed -i '' -E "s/^version = \"[0-9.]+\"/version = \"$VERSION\"/" Cargo.toml
./scripts/bundle.sh

rm -f "$ZIP"
ditto -c -k --keepParent dist/Dusk.app "$ZIP"

# Signature EdDSA avec la clé du trousseau (sparkle:edSignature="…" length="…").
SIGNATURE=$(vendor/Sparkle/bin/sign_update "$ZIP")

python3 - "$VERSION" "$TAG" "$REPO" "$SIGNATURE" <<'PY'
import sys, pathlib, email.utils
version, tag, repo, signature = sys.argv[1:]
path = pathlib.Path("appcast.xml")
if not path.exists():
    path.write_text(
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">\n'
        "  <channel>\n"
        "    <title>Dusk</title>\n"
        "  </channel>\n"
        "</rss>\n"
    )
text = path.read_text()
if f"<sparkle:version>{version}</sparkle:version>" in text:
    sys.exit(f"appcast.xml contient déjà {version}")
item = f"""    <item>
      <title>Dusk {version}</title>
      <link>https://github.com/{repo}/releases/tag/{tag}</link>
      <pubDate>{email.utils.formatdate(usegmt=True)}</pubDate>
      <sparkle:version>{version}</sparkle:version>
      <sparkle:shortVersionString>{version}</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>14.0</sparkle:minimumSystemVersion>
      <enclosure url="https://github.com/{repo}/releases/download/{tag}/Dusk.zip" type="application/octet-stream" {signature}/>
    </item>
"""
# La plus récente en premier, juste après le titre du canal.
marker = "    <title>Dusk</title>\n"
path.write_text(text.replace(marker, marker + item, 1))
PY

gh release create "$TAG" "$ZIP" --repo "$REPO" --draft --title "Dusk $VERSION" --notes ""
echo "Brouillon $TAG prêt. Ensuite : publier le brouillon, commit, ./scripts/publish.sh, push."
