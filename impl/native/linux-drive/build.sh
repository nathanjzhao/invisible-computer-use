#!/usr/bin/env bash
# Build the patched cua-driver for Linux natively inside Docker (the host's arch; colima on Apple
# silicon gives arm64). cargo-zigbuild cannot cross-build this crate (x11 pkg-config), so the build
# runs in rust:1.97-bookworm with the X11/D-Bus dev packages, caching the registry and the target
# dir under vendor-cua/libs/cua-driver/rust/target/docker-*. The whole repo is mounted because the
# vendored crates depend on impl/axdiff by path.
#   impl/native/linux-drive/build.sh            -> target/docker-linux/release/cua-driver and impl/target/docker-linux/release/icu
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
RUST="$ROOT/vendor-cua/libs/cua-driver/rust"
mkdir -p "$RUST/target/docker-linux" "$RUST/target/docker-cargo-home"
docker run --rm \
  -v "$ROOT:/src" \
  -v "$RUST/target/docker-cargo-home:/usr/local/cargo/registry" \
  -w /src/vendor-cua/libs/cua-driver/rust \
  rust:1.97-bookworm bash -c '
    set -e
    apt-get update -qq >/dev/null
    apt-get install -y -qq --no-install-recommends pkg-config libx11-dev libxi-dev libxtst-dev libxext-dev \
      libxrandr-dev libxfixes-dev libxcomposite-dev libxrender-dev libssl-dev libdbus-1-dev clang libclang-dev \
      libasound2-dev libglib2.0-dev libpipewire-0.3-dev >/dev/null
    cargo build --release -p cua-driver --target-dir target/docker-linux
    ls -la target/docker-linux/release/cua-driver
    cd /src/impl && cargo build --release -p icu --target-dir target/docker-linux
    ls -la target/docker-linux/release/icu'
