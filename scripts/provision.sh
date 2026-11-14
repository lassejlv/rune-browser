#!/usr/bin/env bash
# Gets the provisioning profile scripts/bundle.sh signs with, so Touch ID passkeys work.
#   scripts/provision.sh <team id>
# Xcode must be signed in to an Apple Developer account in that team (Settings → Accounts).
# This registers the Mac as a development device in the team and leaves a "Mac Team
# Provisioning Profile" in .signing/, good for a year. Needs xcodegen (brew install xcodegen).
set -euo pipefail
cd "$(dirname "$0")/.."
team=${1:?usage: scripts/provision.sh <team id>}
work=target/provision
rm -rf "$work" && mkdir -p "$work/Sources" .signing
echo 'print("provisioning stub")' > "$work/Sources/main.swift"
cat > "$work/stub.entitlements" <<'ENTITLEMENTS'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>keychain-access-groups</key>
  <array><string>$(AppIdentifierPrefix)runebrowser.webauthn</string></array>
</dict></plist>
ENTITLEMENTS
cat > "$work/project.yml" <<YML
name: Provision
settings:
  base:
    DEVELOPMENT_TEAM: $team
    CODE_SIGN_STYLE: Automatic
    GENERATE_INFOPLIST_FILE: YES
    MACOSX_DEPLOYMENT_TARGET: "12.0"
targets:
  Stub:
    type: application
    platform: macOS
    sources: [Sources]
    settings:
      base:
        PRODUCT_BUNDLE_IDENTIFIER: app.runebrowser
        CODE_SIGN_ENTITLEMENTS: stub.entitlements
YML
(cd "$work" && xcodegen generate >/dev/null && xcodebuild -project Provision.xcodeproj -scheme Stub -derivedDataPath build -allowProvisioningUpdates -allowProvisioningDeviceRegistration build | grep -E "error:|BUILD")
cp "$work/build/Build/Products/Debug/Stub.app/Contents/embedded.provisionprofile" .signing/mac.provisionprofile
echo ".signing/mac.provisionprofile"
