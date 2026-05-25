#!/bin/bash
# Build the phototrail tool. Run on macOS.
set -euo pipefail

cd "$(dirname "$0")"

echo "Compiling phototrail..."
swiftc -O phototrail.swift -o phototrail \
    -framework Photos \
    -framework CoreLocation \
    -framework ImageIO \
    -Xlinker -sectcreate \
    -Xlinker __TEXT \
    -Xlinker __info_plist \
    -Xlinker Info.plist

# Ad-hoc code sign so macOS can attribute and persist the Photos permission grant.
echo "Code signing (ad-hoc)..."
codesign --force --sign - phototrail

echo "Done. Run ./phototrail help to get started."
