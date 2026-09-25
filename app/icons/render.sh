#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
tray=../src-tauri/icons/tray
mkdir -p "$tray"
for state in idle indexing-1 indexing-2 indexing-3 update; do
  rsvg-convert -w 64 -h 64 "tray-$state.svg" -o "$tray/$state-black.png"
  sed 's/#000000/#FFFFFF/gI' "tray-$state.svg" | rsvg-convert -w 64 -h 64 -o "$tray/$state-white.png"
done
work=$(mktemp -d)
rsvg-convert -w 1024 -h 1024 app-icon.svg -o "$work/app-icon.png"
(cd .. && pnpm tauri icon "$work/app-icon.png" > /dev/null 2>&1)
rm -rf "$work" ../src-tauri/icons/android ../src-tauri/icons/ios
