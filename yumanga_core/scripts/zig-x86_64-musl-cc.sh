#!/bin/bash
set -eu

filtered_args=()
for argument in "$@"; do
	case "$argument" in
		--target=*|-target=*) continue ;;
		*) filtered_args+=("$argument") ;;
	esac
done

exec zig cc -target x86_64-linux-musl -nostartfiles "${filtered_args[@]}"