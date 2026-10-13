#!/usr/bin/env bash
# Builds the release binaries and packs them for download (used by .github/workflows):
#
#     tools/dist.sh NAME      dist/NAME.tar.gz and dist/NAME.tar.gz.sha256
#
# The archive has a NAME/ folder with apex5, apex5ctl, packaging/, LICENSES/ and the READMEs.
set -euo pipefail

name=${1:?usage: tools/dist.sh NAME}
cd "$(dirname "$0")/.."

cargo build --release --locked

stage=dist/$name
rm -rf "$stage" "$stage.tar.gz" "$stage.tar.gz.sha256"
mkdir -p "$stage"
install -m755 target/release/apex5 target/release/apex5ctl "$stage/"
strip "$stage/apex5" "$stage/apex5ctl"
cp -r packaging LICENSES README.md README.ru.md "$stage/"

tar -C dist -czf "dist/$name.tar.gz" "$name"
(cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
rm -rf "$stage"
echo "dist/$name.tar.gz"
