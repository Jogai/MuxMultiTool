#!/usr/bin/env bash
# Downloads the ffmpeg and ffprobe sidecars for one target triple into
# crates/desktop/binaries/, named the way Tauri's `bundle.externalBin` expects
# (`ffmpeg-<triple>[.exe]`). CI runs it before `tauri build`; run it by hand
# for a local build. Every archive is pinned by URL and SHA-256, so a build is
# reproducible and a changed download fails loudly; Renovate does not track
# these, bump them by hand.
#
# Sources (architecture.md, "ffmpeg sources"): all GPL builds with x264.
# - Windows: BtbN FFmpeg-Builds, `win64-gpl` (static, --enable-gpl --enable-version3).
# - Linux: BtbN FFmpeg-Builds, `linux64-gpl` and `linuxarm64-gpl` from the same
#   autobuild (glibc, like the desktop binary itself; the container uses a
#   different, musl build).
# - macOS: Martin Riedl's static builds (https://ffmpeg.martin-riedl.de), the
#   `release` channel, for arm64 and amd64 (--enable-gpl --enable-version3
#   --enable-libx264; OpenSSL 3 is Apache-2.0, which GPL-3.0 allows).
#
# Usage: fetch-sidecars.sh <target-triple>
set -euo pipefail

target="${1:-}"
case "$target" in
	x86_64-pc-windows-msvc | x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu | aarch64-apple-darwin | x86_64-apple-darwin) ;;
	*)
		echo "usage: $0 x86_64-pc-windows-msvc | x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu | aarch64-apple-darwin | x86_64-apple-darwin" >&2
		exit 64
		;;
esac

here="$(cd "$(dirname "$0")" && pwd)"
out="$here/../binaries"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"

btbn_tag="autobuild-2026-10-27-18-38"
btbn_build="ffmpeg-n8.1.3-24-g92611900bb"
btbn_win64="$btbn_build-win64-gpl-8.1"
btbn_win64_sha256="79a803e134ffbd69c08b380aac33693dc028135785b23213f292d84c59c1e95e"
btbn_linux64="$btbn_build-linux64-gpl-8.1"
btbn_linux64_sha256="292e10a463befababcd31e0c7d0c1e068d46342398d9674534eb07517c1ba38e"
btbn_linuxarm64="$btbn_build-linuxarm64-gpl-8.1"
btbn_linuxarm64_sha256="f6cddb0f449e2db8dd89e8c9407d972331793a5f129065c5db7a94dacd4270e7"

riedl="https://ffmpeg.martin-riedl.de/download/macos"
riedl_arm64="arm64/1789931890_9.0.2"
riedl_arm64_ffmpeg_sha256="c8ed4c4e6978a03c485edbfe4e0a5dc2380f8a30bba5150531b31b094492d924"
riedl_arm64_ffprobe_sha256="fcbe839537485eaee7a7a8bc5cbc0f90d53617e80943e8a5b2e31cb851197ea6"
riedl_amd64="amd64/1789931006_9.0.2"
riedl_amd64_ffmpeg_sha256="7c6b4125b191cbf773832dc51f424cf2b6bb7da43007d1e066f95909e47cacd4"
riedl_amd64_ffprobe_sha256="2322438ed2f6319a691291b247d09c69dcaa3a982460d1f269a7e1af335cfdfd"

# fetch <url> <sha256> <file>
fetch() {
	echo "downloading $1"
	curl --fail --location --silent --show-error --retry 3 --output "$3" "$1"
	local actual
	if command -v sha256sum >/dev/null; then
		actual="$(sha256sum "$3" | cut -d ' ' -f 1)"
	else
		actual="$(shasum -a 256 "$3" | cut -d ' ' -f 1)"
	fi
	if [ "$actual" != "$2" ]; then
		echo "checksum mismatch for $1: expected $2, got $actual" >&2
		exit 1
	fi
}

# unzip_into <archive> <folder>: unzip on macOS and Linux, 7-Zip on the Windows runners
unzip_into() {
	if command -v unzip >/dev/null; then
		unzip -q "$1" -d "$2"
	else
		7z x -y -o"$2" "$1" >/dev/null
	fi
}

case "$target" in
	x86_64-pc-windows-msvc)
		fetch "https://github.com/BtbN/FFmpeg-Builds/releases/download/$btbn_tag/$btbn_win64.zip" \
			"$btbn_win64_sha256" "$work/win64.zip"
		unzip_into "$work/win64.zip" "$work/win64"
		for tool in ffmpeg ffprobe; do
			cp "$work/win64/$btbn_win64/bin/$tool.exe" "$out/$tool-$target.exe"
		done
		;;
	x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu)
		if [ "$target" = x86_64-unknown-linux-gnu ]; then
			build="$btbn_linux64" sha="$btbn_linux64_sha256"
		else
			build="$btbn_linuxarm64" sha="$btbn_linuxarm64_sha256"
		fi
		fetch "https://github.com/BtbN/FFmpeg-Builds/releases/download/$btbn_tag/$build.tar.xz" \
			"$sha" "$work/linux.tar.xz"
		mkdir -p "$work/linux"
		tar -xJf "$work/linux.tar.xz" -C "$work/linux"
		for tool in ffmpeg ffprobe; do
			cp "$work/linux/$build/bin/$tool" "$out/$tool-$target"
			chmod +x "$out/$tool-$target"
		done
		;;
	aarch64-apple-darwin | x86_64-apple-darwin)
		if [ "$target" = aarch64-apple-darwin ]; then
			dir="$riedl_arm64" ffmpeg_sha="$riedl_arm64_ffmpeg_sha256" ffprobe_sha="$riedl_arm64_ffprobe_sha256"
		else
			dir="$riedl_amd64" ffmpeg_sha="$riedl_amd64_ffmpeg_sha256" ffprobe_sha="$riedl_amd64_ffprobe_sha256"
		fi
		for tool in ffmpeg ffprobe; do
			sha="$ffmpeg_sha"
			[ "$tool" = ffprobe ] && sha="$ffprobe_sha"
			fetch "$riedl/$dir/$tool.zip" "$sha" "$work/$tool.zip"
			unzip_into "$work/$tool.zip" "$work/$tool"
			cp "$work/$tool/$tool" "$out/$tool-$target"
			chmod +x "$out/$tool-$target"
		done
		;;
esac

echo "sidecars for $target in $out:"
ls -l "$out"
