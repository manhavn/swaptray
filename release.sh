#!/usr/bin/env bash
# Build every release variant of swaptray in one command, ready to upload
# to a GitHub Release for installation via:
#   mise use -g github:manhavn/swaptray
#
# Usage:
#   ./release.sh            # version taken from Cargo.toml
#   ./release.sh 0.2.0      # explicit version
#
# Backends (first available wins):
#   1. cargo-zigbuild + zig  -> no docker, builds all 4 targets
#   2. cross                 -> needs docker/podman
set -euo pipefail
cd "$(dirname "$0")"

NAME=swaptray
PACKAGE_VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
VERSION="${1:-$PACKAGE_VERSION}"
if [ "$VERSION" != "$PACKAGE_VERSION" ]; then
  echo "error: update Cargo.toml version to $VERSION before building that release" >&2
  exit 1
fi
DIST=dist

# rust targets: glibc builds get a 2.17 floor (works on old distros),
# musl builds use the native musl loader to support GUI library dlopen.
GNU_TARGETS=(x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu)
MUSL_TARGETS=(x86_64-unknown-linux-musl aarch64-unknown-linux-musl)
GLIBC_FLOOR=2.17

if command -v cargo-zigbuild >/dev/null 2>&1 && command -v zig >/dev/null 2>&1; then
  BACKEND=zigbuild
  # rustc adds -Wl,-O1; Zig ignores this deprecated ELF linker option.
  # Adapt the input argument, rather than silencing linker diagnostics.
  export SWAPTRAY_REAL_ZIG="$(command -v zig)"
  export CARGO_ZIGBUILD_ZIG_COMMAND="$(pwd)/scripts/zig-linker.py"
  export CARGO_ZIGBUILD_ZIG_COMMAND_ARGS=""
  export CARGO_ZIGBUILD_ZIG_VERSION
  CARGO_ZIGBUILD_ZIG_VERSION=$("$SWAPTRAY_REAL_ZIG" version)
elif command -v cross >/dev/null 2>&1; then
  BACKEND=cross
else
  echo "error: no cross-compilation backend found." >&2
  echo "  install one of:" >&2
  echo "    cargo install cargo-zigbuild   (zig must be on PATH, e.g. via mise)" >&2
  echo "    https://github.com/cross-rs/cross (needs docker/podman)" >&2
  exit 1
fi

echo "backend : $BACKEND"
echo "version : $VERSION"

# Host build scripts need a C linker. Fall back to clang when cc is absent
# (minimal/mise-managed environments); cross targets are linked by zig/cross.
if ! command -v cc >/dev/null 2>&1; then
  if command -v clang >/dev/null 2>&1; then
    linker_dir=$(mktemp -d)
    ln -s "$(command -v clang)" "$linker_dir/cc"
    export PATH="$linker_dir:$PATH"
    trap 'rm -rf "$linker_dir"' EXIT
    echo "note    : no cc found, using clang as host linker"
  else
    echo "error: no C linker found (install gcc or clang)" >&2
    exit 1
  fi
fi

# rust std libraries for the targets (base triples; zigbuild adds the floor suffix)
rustup target add \
  "${GNU_TARGETS[@]}" \
  "${MUSL_TARGETS[@]}" >/dev/null

rm -rf "$DIST"
mkdir -p "$DIST"

build_target() {
  local target=$1 rust_target=$2
  echo "==> building $target"
  case "$BACKEND" in
    zigbuild)
      cargo zigbuild --release --target "$rust_target"
      ;;
    cross)
      cross build --release --target "$target"
      ;;
  esac
}

find_binary() {
  # zigbuild may place output under the suffixed target directory
  local target=$1 rust_target=$2
  for dir in "target/$rust_target/release" "target/$target/release"; do
    if [ -x "$dir/$NAME" ]; then
      echo "$dir/$NAME"
      return 0
    fi
  done
  return 1
}

package() {
  local target=$1 binary=$2
  local stage="$DIST/stage-$target"
  local asset="$NAME-v$VERSION-$target.tar.gz"
  mkdir -p "$stage"
  cp "$binary" "$stage/$NAME"
  tar -C "$stage" -czf "$DIST/$asset" "$NAME"
  rm -rf "$stage"
  (cd "$DIST" && sha256sum "$asset" > "$asset.sha256")
  echo "    packaged $asset"
}

for target in "${GNU_TARGETS[@]}"; do
  if [ "$BACKEND" = zigbuild ]; then
    rust_target="$target.$GLIBC_FLOOR"
    build_target "$target" "$rust_target"
  else
    rust_target="$target"
    build_target "$target" "$rust_target"
  fi
  binary=$(find_binary "$target" "$rust_target") || { echo "error: binary not found for $target" >&2; exit 1; }
  file "$binary" | sed 's/^/    /'
  package "$target" "$binary"
done

for target in "${MUSL_TARGETS[@]}"; do
  rust_target="$target"
  arch=${target%%-*}
  runtime="$(pwd)/$DIST/musl-$arch.so"
  python3 scripts/musl-runtime.py "$arch" "$runtime"
  RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=-crt-static -C link-arg=$runtime -C link-arg=-Wl,--dynamic-linker=/lib/ld-musl-$arch.so.1" build_target "$target" "$rust_target"
  binary=$(find_binary "$target" "$rust_target") || { echo "error: binary not found for $target" >&2; exit 1; }
  file "$binary" | sed 's/^/    /'
  package "$target" "$binary"
  rm -f "$runtime"
done

(cd "$DIST" && cat ./*.sha256 > SHA256SUMS.txt)

cat <<EOF

All variants built in $DIST/:
$(ls -1 "$DIST"/*.tar.gz | sed 's|^|  |')

Upload manually to a GitHub Release:

  gh release create "v$VERSION" $DIST/*.tar.gz $DIST/SHA256SUMS.txt \\
    --title "v$VERSION" --notes "swaptray v$VERSION"

Then install with:

  mise use -g github:manhavn/swaptray
EOF
