#!/usr/bin/env bash
# Downloads the Chromium Embedded Framework build the `cef` crate expects into .cef/.
#
# The crate's own downloader reads CEF's whole build index through an HTTP client capped at
# 10 MB, and the index has outgrown that, so the build fails before it downloads anything.
# This fetches the same archive with curl and lays it out the way the crate would.
set -euo pipefail
cd "$(dirname "$0")/.."

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) platform=macosarm64; dir=cef_macos_aarch64 ;;
  Darwin-x86_64) platform=macosx64; dir=cef_macos_x86_64 ;;
  *) echo "fetch-cef: only macOS is set up so far" >&2; exit 1 ;;
esac

# cef = "156.4.0+156.0.26": the part after the plus is the CEF release.
version=$(awk '/^name = "cef"$/ {getline; gsub(/version = |"/, ""); print; exit}' Cargo.lock)
cef_version=${version#*+}
dest=".cef/$cef_version/$dir"
if [ -f "$dest/archive.json" ]; then
  echo "fetch-cef: CEF $cef_version is already in $dest"
  exit 0
fi

cdn=${CEF_DOWNLOAD_URL:-https://cef-builds.spotifycdn.com}
work=".cef/$cef_version/download"
mkdir -p "$work"
echo "fetch-cef: reading the CEF build index"
curl -fsSL "$cdn/index.json" -o "$work/index.json"
read -r name sha1 < <(python3 - "$work/index.json" "$platform" "$cef_version" <<'PY'
import json, sys
index, platform, version = json.load(open(sys.argv[1])), sys.argv[2], sys.argv[3]
for build in index[platform]["versions"]:
    if build["cef_version"].startswith(version + "+"):
        for f in build["files"]:
            if f["type"] == "minimal":
                print(f["name"], f["sha1"])
                sys.exit(0)
sys.exit(f"no minimal build of CEF {version} for {platform}")
PY
)

echo "fetch-cef: downloading $name"
curl -fL --retry 3 -C - "$cdn/$(python3 -c 'import sys, urllib.parse; print(urllib.parse.quote(sys.argv[1]))' "$name")" -o "$work/$name"
echo "$sha1  $work/$name" | shasum -a 1 -c - >/dev/null || { echo "fetch-cef: checksum mismatch" >&2; exit 1; }

echo "fetch-cef: extracting"
tar -xjf "$work/$name" -C "$work"
src="$work/${name%.tar.bz2}"
rm -rf "$dest"
mv "$src/Release" "$dest"
for part in CMakeLists.txt cmake include libcef_dll CREDITS.html; do mv "$src/$part" "$dest/$part"; done
printf '{\n  "type": "minimal",\n  "name": "%s",\n  "sha1": "%s"\n}' "$name" "$sha1" > "$dest/archive.json"
rm -rf "$work"
echo "fetch-cef: CEF $cef_version is ready in $dest"
