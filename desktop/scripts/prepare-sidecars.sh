#!/usr/bin/env bash
# Stage everything tauri.conf.json bundles into the .app, built from this
# flake's pinned inputs:
#   binaries/<name>-<triple>  each `externalBin` sidecar (pdftotext, node) from
#                             nixpkgs, its dylibs relocated into binaries/libs/
#                             with install names pointing at
#                             @executable_path/../Resources/libs/
#   binaries/agent-runner/    the Agent Ask runner (.#agent-runner)
#   ../frontend/dist/         the UI rust-embed compiles in (.#frontend)
# install_name_tool invalidates code signatures and unsigned arm64 binaries
# are killed on launch, so everything touched is re-signed ad-hoc.
#
# `--stub` writes empty stand-ins instead, for builds that never run them:
# tauri-build only checks that the sidecars and resources exist.
set -euo pipefail
cd "$(dirname "$0")/.."

TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
SIDECARS=$(jq -r '.bundle.externalBin[]' tauri.conf.json)
rm -rf binaries
mkdir -p binaries/libs binaries/agent-runner

if [ "${1:-}" = --stub ]; then
  for sidecar in $SIDECARS; do
    touch "${sidecar}-${TRIPLE}"
  done
  exit 0
fi

ROOT=$(cd .. && pwd)
build() {
  nix build --no-link --print-out-paths --inputs-from "$ROOT" "$@"
}

echo "==> frontend and agent runner"
rm -rf "$ROOT/frontend/dist"
cp -RL "$(build "$ROOT#frontend")" "$ROOT/frontend/dist"
cp -RL "$(build "$ROOT#agent-runner")/lib/xuewen/agent-runner/." binaries/agent-runner/
chmod -R u+w "$ROOT/frontend/dist" binaries/agent-runner

echo "==> sidecars (${TRIPLE})"
cp "$(build 'nixpkgs#poppler-utils^out')/bin/pdftotext" "binaries/pdftotext-${TRIPLE}"
cp "$(build 'nixpkgs#nodejs')/bin/node" "binaries/node-${TRIPLE}"
DYLIBBUNDLER="$(build 'nixpkgs#macdylibbundler')/bin/dylibbundler"
for sidecar in $SIDECARS; do
  chmod u+w "${sidecar}-${TRIPLE}"
  # </dev/null turns any interactive "library not found" prompt into a
  # failure instead of a hang on CI.
  "$DYLIBBUNDLER" -of -b \
    -x "${sidecar}-${TRIPLE}" \
    -d binaries/libs \
    -p '@executable_path/../Resources/libs/' \
    </dev/null
done

echo "==> ad-hoc re-sign"
for sidecar in $SIDECARS; do
  codesign --force -s - "${sidecar}-${TRIPLE}"
done
for f in binaries/libs/*.dylib; do
  codesign --force -s - "$f"
done

echo "==> done:"
ls -l binaries binaries/libs
