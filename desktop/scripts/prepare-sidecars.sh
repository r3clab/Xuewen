#!/usr/bin/env bash
# Prepare the sidecar binaries Tauri bundles into the .app:
#   binaries/node-<triple>       official Node.js (Agent Ask runner)
#   binaries/pdftotext-<triple>  Homebrew poppler; dylibs relocated into
#                                binaries/libs/ with install names pointing at
#                                @executable_path/../Resources/libs/
# install_name_tool invalidates code signatures and unsigned arm64 binaries
# are killed on launch, so everything touched is re-signed ad-hoc.
set -euo pipefail
cd "$(dirname "$0")/.."

TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
NODE_VERSION="${NODE_VERSION:-24.18.0}"
# The download must match the triple the outputs are named after: a wrong-arch
# node still passes tauri-build's existence check and only fails inside the
# shipped .app, at runtime, with "Bad CPU type in executable".
case "$TRIPLE" in
  aarch64-apple-darwin) NODE_ARCH=darwin-arm64 ;;
  x86_64-apple-darwin) NODE_ARCH=darwin-x64 ;;
  *)
    echo "unsupported host: $TRIPLE" >&2
    exit 1
    ;;
esac
mkdir -p binaries/libs

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

NODE_DIST="node-v${NODE_VERSION}-${NODE_ARCH}"
echo "==> ${NODE_DIST} (${TRIPLE})"
curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/${NODE_DIST}.tar.gz" \
  -o "${WORK}/${NODE_DIST}.tar.gz"
curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/SHASUMS256.txt" \
  -o "${WORK}/SHASUMS256.txt"
(cd "$WORK" && grep " ${NODE_DIST}.tar.gz\$" SHASUMS256.txt | shasum -a 256 -c -)
tar -xzf "${WORK}/${NODE_DIST}.tar.gz" -C "$WORK"
cp "${WORK}/${NODE_DIST}/bin/node" "binaries/node-${TRIPLE}"

echo "==> pdftotext from $(brew --prefix poppler)"
cp "$(brew --prefix poppler)/bin/pdftotext" "binaries/pdftotext-${TRIPLE}"
chmod u+w "binaries/pdftotext-${TRIPLE}"
# -s gives dylibbundler the keg-only lib dirs to resolve deps like
# libpoppler.NNN.dylib; without them it prompts interactively ("does not
# exist. Try again") and, with no stdin on CI, loops forever. </dev/null makes
# any remaining prompt hit EOF and fail fast instead of hanging.
dylibbundler -of -b \
  -x "binaries/pdftotext-${TRIPLE}" \
  -d binaries/libs \
  -p '@executable_path/../Resources/libs/' \
  -s "$(brew --prefix poppler)/lib" \
  -s "$(brew --prefix)/lib" \
  </dev/null

echo "==> ad-hoc re-sign"
codesign --force -s - "binaries/node-${TRIPLE}" "binaries/pdftotext-${TRIPLE}"
for f in binaries/libs/*.dylib; do
  codesign --force -s - "$f"
done

echo "==> done:"
ls -l binaries binaries/libs
