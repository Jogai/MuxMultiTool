#!/bin/sh
# Generates the test clip as a VOD HLS stream: 60 s of testsrc2 (shows a running
# clock, so seeking is visible) with a 440 Hz sine tone, H.264 + AAC in MPEG-TS
# segments of 4 s. Output goes to clip/ next to this script.
set -eu
cd "$(dirname "$0")"
rm -rf clip
mkdir -p clip
ffmpeg -hide_banner -loglevel error \
	-f lavfi -i "testsrc2=size=640x360:rate=30" \
	-f lavfi -i "sine=frequency=440:sample_rate=48000" \
	-t 60 \
	-c:v libx264 -preset veryfast -profile:v main -pix_fmt yuv420p \
	-g 120 -keyint_min 120 -sc_threshold 0 \
	-c:a aac -b:a 96k -ac 2 \
	-f hls -hls_time 4 -hls_playlist_type vod \
	-hls_flags independent_segments \
	-hls_segment_filename "clip/%03d.ts" \
	clip/index.m3u8
echo "clip/index.m3u8 with $(ls clip/*.ts | wc -l) segments"
