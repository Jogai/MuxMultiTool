#!/usr/bin/env bash
# Downloads the ffmpeg and ffprobe sidecars for one target triple into
# crates/desktop/binaries/, named the way Tauri's `bundle.externalBin` expects
# (`ffmpeg-<triple>[.exe]`). CI runs it before `tauri build`; run it by hand
# for a local build. Every archive is pinned by URL and SHA-256, so a build is
# reproducible and a changed download fails loudly; Renovate does not track
# these, bump them by hand.
#
# Sources (architecture.md, "ffmpeg sources"): all GPL-3.0 builds with x264,
# none of them configured with --enable-nonfree.
# - Windows: gyan.dev's release "essentials" build (static, GPL v3), from its
#   versioned GitHub release (GyanD/codexffmpeg).
# - Linux: mwader/static-ffmpeg, the build the container ships (static musl,
#   --enable-gpl --enable-version3), taken from the image layer of each
#   architecture. The layer is pinned by its digest, which is its SHA-256.
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

gyan_build="ffmpeg-9.1-essentials_build"
gyan_url="https://github.com/GyanD/codexffmpeg/releases/download/9.1/$gyan_build.zip"
gyan_sha256="b9bbbb3acece000cbeec994f856649354e52c773807841e78908374b24d658fd"

# mwader/static-ffmpeg:9.1 (the FFMPEG_IMAGE of the Dockerfile): the one layer
# of the amd64 and of the arm64 image, an uncompressed tar with ffmpeg and
# ffprobe at its root.
static_image="mwader/static-ffmpeg"
static_amd64_layer="92b424de5640d180909a9900705ff37b58642b07685e26c5a3fdea0b455201d2"
static_arm64_layer="8c7e5f24811c61755f475e043e7ccc799f7b4db7f9b81309afc4b7bff987932b"

riedl="https://ffmpeg.martin-riedl.de/download/macos"
riedl_arm64="arm64/1795207104_9.1"
riedl_arm64_ffmpeg_sha256="1f66e9a690a7f1c9e60b012379f8869c11818d1797f45b21dbb2f2564ba61ae1"
riedl_arm64_ffprobe_sha256="6ef2df5f575042923a28537ab37adf936f28bf3e7b85de23a1447ed447ec1dbd"
riedl_amd64="amd64/1795212433_9.1"
riedl_amd64_ffmpeg_sha256="35b3d82cccc2c1d644d49d15f5e57eb913a08ef68f798042c90eecec62c51f1e"
riedl_amd64_ffprobe_sha256="41ca7b7cba7992e33f1da3f1bf54db8ec103a4d387b1bfac3162062e8b1074ad"

# fetch <url> <sha256> <file> [curl options...]
fetch() {
	echo "downloading $1"
	curl --fail --location --silent --show-error --retry 3 --output "$3" "${@:4}" "$1"
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
		fetch "$gyan_url" "$gyan_sha256" "$work/win64.zip"
		unzip_into "$work/win64.zip" "$work/win64"
		for tool in ffmpeg ffprobe; do
			cp "$work/win64/$gyan_build/bin/$tool.exe" "$out/$tool-$target.exe"
		done
		;;
	x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu)
		layer="$static_amd64_layer"
		[ "$target" = aarch64-unknown-linux-gnu ] && layer="$static_arm64_layer"
		# Docker Hub serves public blobs with an anonymous pull token.
		token="$(curl --fail --silent --show-error --retry 3 \
			"https://auth.docker.io/token?service=registry.docker.io&scope=repository:$static_image:pull" |
			sed -E 's/.*"token":"([^"]+)".*/\1/')"
		fetch "https://registry-1.docker.io/v2/$static_image/blobs/sha256:$layer" \
			"$layer" "$work/layer.tar" --header "Authorization: Bearer $token"
		mkdir -p "$work/linux"
		tar -xf "$work/layer.tar" -C "$work/linux" ffmpeg ffprobe
		for tool in ffmpeg ffprobe; do
			cp "$work/linux/$tool" "$out/$tool-$target"
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
