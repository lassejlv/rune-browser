#!/usr/bin/env bash
# Assembles "Rune Browser.app" from already-built binaries: Chromium only runs from a macOS app
# bundle, with its framework and five helper apps inside.
#   scripts/bundle.sh [debug|release]
# NAME="Rune Browser Dev" makes a second app with a profile of its own, to test beside the
# one in use (the name must start with "Rune Browser").
set -euo pipefail
cd "$(dirname "$0")/.."

profile=${1:-debug}
name=${NAME:-Rune Browser}
id="app.runebrowser$(echo "${name#Rune Browser}" | tr -d ' ' | tr '[:upper:]' '[:lower:]' | sed 's/^./.&/')"
version=$(awk -F'"' '/^version/ {print $2; exit}' Cargo.toml)
out="target/$profile"
app="$out/$name.app"
framework="Chromium Embedded Framework.framework"
cef=$(ls -d .cef/*/cef_macos_* 2>/dev/null | head -1)
[ -d "$cef/$framework" ] || { echo "bundle: no CEF in .cef/ (run scripts/fetch-cef.sh)" >&2; exit 1; }

plist() { # plist <path> <executable> <identifier> <helper: 0|1>
  cat > "$1" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$name</string>
  <key>CFBundleDisplayName</key><string>$name</string>
  <key>CFBundleExecutable</key><string>$2</string>
  <key>CFBundleIdentifier</key><string>$3</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>LSEnvironment</key><dict><key>MallocNanoZone</key><string>0</string></dict>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>NSCameraUsageDescription</key><string>A website wants to use your camera.</string>
  <key>NSMicrophoneUsageDescription</key><string>A website wants to use your microphone.</string>
  <key>NSLocationUsageDescription</key><string>A website wants to know your location.</string>
  <key>NSBluetoothAlwaysUsageDescription</key><string>Signing in with a passkey on your phone uses Bluetooth to check the phone is nearby.</string>
$( [ "$4" = 1 ] && echo "  <key>LSUIElement</key><string>1</string>" )
$( [ "$4" = 0 ] && cat <<URLS
  <key>CFBundleURLTypes</key>
  <array><dict>
    <key>CFBundleURLName</key><string>Web site URL</string>
    <key>CFBundleURLSchemes</key><array><string>http</string><string>https</string></array>
  </dict></array>
URLS
)
</dict>
</plist>
PLIST
}

mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks" "$app/Contents/Resources"
plist "$app/Contents/Info.plist" "$name" "$id" 0
# Binaries are put in place by rename, never written over: a copy of the app that is running
# keeps the file it started from.
install_binary() { cp -f "$1" "$2.new" && mv -f "$2.new" "$2"; }
install_binary "$out/rune-browser" "$app/Contents/MacOS/$name"

# The framework is ~300 MB: copy it once per CEF version (cp -c clones on APFS, taking no space).
# The stamps sit beside the app: a signed bundle may hold only code in Frameworks.
mkdir -p "$out/$name.stamps"
rm -f "$app/Contents/Frameworks/.cef-version" "$app/Contents/Frameworks/.signed-by"
stamp="$out/$name.stamps/cef-version"
if [ "$(cat "$stamp" 2>/dev/null)" != "$cef" ]; then
  rm -rf "$app/Contents/Frameworks/$framework" "$out/$name.stamps/signed-by"
  cp -Rc "$cef/$framework" "$app/Contents/Frameworks/" 2>/dev/null || cp -R "$cef/$framework" "$app/Contents/Frameworks/"
  echo "$cef" > "$stamp"
fi

for kind in "" " (GPU)" " (Renderer)" " (Plugin)" " (Alerts)"; do
  helper="$name Helper$kind"
  suffix=$(echo "$kind" | tr -d ' ()' | tr '[:upper:]' '[:lower:]')
  dir="$app/Contents/Frameworks/$helper.app/Contents"
  mkdir -p "$dir/MacOS"
  plist "$dir/Info.plist" "$helper" "$id.helper${suffix:+.$suffix}" 1
  install_binary "$out/rune-browser-helper" "$dir/MacOS/$helper"
done

# Signing, when scripts/provision.sh has left a provisioning profile in .signing/ (SIGN=0 skips
# it). Chromium keeps Touch ID passkeys in a keychain access group, which macOS grants only
# to an app signed by a developer team with a profile that allows it. The group's name is
# compiled into the framework as ".org.chromium.Chromium.webauthn", which no team can be
# granted (a group starts with the team's id), so the bundle's copy of the framework is
# given the team's group of the same length instead.
mobileprovision=.signing/mac.provisionprofile
if [ -f "$mobileprovision" ] && [ "${SIGN:-1}" != 0 ]; then
  read -r team identity <<<"$(security cms -D -i "$mobileprovision" 2>/dev/null | python3 -c '
import sys, plistlib, hashlib, subprocess
profile = plistlib.loads(sys.stdin.buffer.read())
have = subprocess.run(["security", "find-identity", "-v", "-p", "codesigning"], capture_output=True, text=True).stdout
hashes = [hashlib.sha1(cert).hexdigest().upper() for cert in profile["DeveloperCertificates"]]
print(profile["TeamIdentifier"][0], next((h for h in hashes if h in have), ""))')"
  [ -n "${identity:-}" ] || { echo "bundle: no signing certificate in the keychain matches $mobileprovision" >&2; exit 1; }
  group="$team.runebrowser.webauthn"
  binary="$app/Contents/Frameworks/$framework/Chromium Embedded Framework"
  sign() { codesign --force --sign "$identity" --timestamp=none "$@" 2>/dev/null; }

  signed="$out/$name.stamps/signed-by"
  if [ "$(cat "$signed" 2>/dev/null)" != "$identity $cef" ]; then
    python3 - "$binary" "$group" <<'PATCH'
import sys
path, group = sys.argv[1], sys.argv[2].encode()
old = b".org.chromium.Chromium.webauthn"
assert len(group) == len(old), "the team's keychain group must be as long as Chromium's"
data = open(path, "rb").read()
if old in data:
    open(path, "wb").write(data.replace(old, group))
elif group not in data:
    sys.exit("bundle: Chromium's passkey keychain group was not found in the framework")
PATCH
    find "$app/Contents/Frameworks/$framework/Libraries" -type f \( -name "*.dylib" -o -perm +111 \) -print0 | while IFS= read -r -d '' library; do sign "$library"; done
    sign "$app/Contents/Frameworks/$framework"
    echo "$identity $cef" > "$signed"
  fi
  for kind in "" " (GPU)" " (Renderer)" " (Plugin)" " (Alerts)"; do sign "$app/Contents/Frameworks/$name Helper$kind.app"; done

  entitlements="$out/$id.entitlements"
  cat > "$entitlements" <<ENTITLEMENTS
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.application-identifier</key><string>$team.$id</string>
  <key>com.apple.developer.team-identifier</key><string>$team</string>
  <key>keychain-access-groups</key>
  <array><string>$group</string><string>$group-uvk</string></array>
</dict>
</plist>
ENTITLEMENTS
  cp -f "$mobileprovision" "$app/Contents/embedded.provisionprofile"
  sign --entitlements "$entitlements" "$app" || { echo "bundle: signing $app failed" >&2; exit 1; }
fi

echo "$app"
