#!/usr/bin/env bash
# Builds, bundles and runs the browser. Arguments after an optional --release go to the app
# (Rune's own, such as --snap out.png).
#   scripts/run.sh [--release]
set -euo pipefail
cd "$(dirname "$0")/.."

profile=debug
flags=()
if [ "${1:-}" = "--release" ]; then profile=release; flags=(--release); shift; fi

./scripts/fetch-cef.sh
cargo build ${flags[@]+"${flags[@]}"} --bin rune-browser --bin rune-browser-helper

if [ "$(uname -s)" = Darwin ]; then
  app=$(./scripts/bundle.sh "$profile")
  # Through LaunchServices, as Finder would: macOS only lets an app use Bluetooth (a passkey on
  # a phone) when the app itself, not the terminal, is what launched it.
  exec open -W "$app" --stdout /dev/stdout --stderr /dev/stderr --args "$@"
fi

# On Linux Chromium (libcef and its data files) sits beside the binaries, where they look for it.
shopt -s nullglob
for part in .cef/*/cef_linux_*/{*.so*,*.pak,*.dat,*.bin,*.json,locales}; do
  cp -Ru "$part" "target/$profile/"
done
exec "target/$profile/rune-browser" "$@"
