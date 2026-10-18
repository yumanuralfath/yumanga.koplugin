#!/bin/sh
set -eu

script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repository_root=$(CDPATH= cd -- "$script_directory/../.." && pwd)
target=x86_64-unknown-linux-musl
linker="$script_directory/zig-x86_64-musl-cc.sh"

command -v zig >/dev/null 2>&1 || {
	echo "Zig is required to build the static Linux KOReader core." >&2
	exit 1
}

rustup target add "$target"
cd "$repository_root"
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
	CC_x86_64_unknown_linux_musl="$linker" \
	cargo build --locked --release --features aidoku-runtime \
		--manifest-path yumanga_core/Cargo.toml --target "$target"

install -D -m 755 \
	"yumanga_core/target/$target/release/yumanga_core" \
	"yumanga.koplugin/bin/yumanga_core_dev"