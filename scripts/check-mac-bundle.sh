#!/bin/sh
# Checks that a built Rimix.app is signed as a bundle.
#
# macOS keys the Local Network permission on the code-signing identifier.
# A bundle left with only the linker's signature has the identifier
# `rimix_desktop-<hash>` instead of the bundle identifier, and macOS then
# drops every connection to the camera's network (and SSDP) with "No route
# to host" while System Settings still shows Rimix as allowed.
#
# Usage: scripts/check-mac-bundle.sh path/to/Rimix.app
set -eu

app="${1:?usage: check-mac-bundle.sh path/to/Rimix.app}"
plist="$app/Contents/Info.plist"
fail=0

bundle_id=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$plist")
signed_id=$(codesign -dv "$app" 2>&1 | sed -n 's/^Identifier=//p')
if [ "$signed_id" != "$bundle_id" ]; then
  echo "FAIL signing identifier is '$signed_id', expected the bundle identifier '$bundle_id'"
  fail=1
else
  echo "ok   signing identifier is $signed_id"
fi

if codesign --verify --deep --strict "$app" 2>/dev/null; then
  echo "ok   signature seals the bundle"
else
  echo "FAIL codesign --verify --deep --strict rejects the bundle"
  fail=1
fi

if /usr/libexec/PlistBuddy -c 'Print :NSLocalNetworkUsageDescription' "$plist" >/dev/null 2>&1; then
  echo "ok   Info.plist says why Rimix needs the local network"
else
  echo "FAIL Info.plist has no NSLocalNetworkUsageDescription"
  fail=1
fi

exit "$fail"
