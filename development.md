# MuxMultiTool development

How to build, run and check MuxMultiTool, and how its services work. For what MuxMultiTool is, see the [readme](readme.md).

## Development

### Backend checks

The Rust workspace lives in `crates/`. `rust-toolchain.toml` pins the Rust version, so `rustup` installs the right toolchain, components and targets on the first `cargo` call. Run these before pushing; CI runs the same commands.

| Check | Command |
|---|---|
| Format | `cargo fmt --all --check` |
| Lint | `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` |
| Docs | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` |
| TOML | `taplo fmt --check` (`cargo install taplo-cli`, or `npx @taplo/cli fmt --check`) |
| Dependencies | `cargo deny check` (`cargo install cargo-deny`): RustSec advisories, licenses, bans and sources, configured in `deny.toml` |
| Unused dependencies | `cargo shear` (`cargo install cargo-shear`) |
| Feature combinations | `cargo hack check --each-feature --workspace --locked` (`cargo install cargo-hack`) |

To fix formatting instead of checking it, drop `--check`. Lints are defined once in the root `Cargo.toml` and every crate inherits them; project rules such as the disallowed `Command::new` (use `core::ffmpeg::Runner`) are in `clippy.toml`. Indentation is tabs everywhere the format allows it (see `.editorconfig`, `rustfmt.toml` and `taplo.toml`).

### Dependency rules

`deny.toml` holds the policy for every crate in the tree:

- **Licenses** must be compatible with GPL-3.0-or-later. The allow-list is the permissive licenses, MPL-2.0 and the GPL/LGPL family; anything else fails the check and needs a per-crate exception with a reason.
- **Bans:** crates that link a C library (`openssl-sys`, `native-tls`, `libz-sys`, `libsqlite3-sys`, ...) are banned because they break the static musl container build. Use rustls and pure-Rust codecs instead. Duplicate versions of a crate and wildcard version requirements are errors; a temporary duplicate after an update goes in `skip` with a reason.
- **Sources:** crates.io only; git dependencies must be listed in `allow-git`.

Release binaries are built with cargo-auditable (`cargo install cargo-auditable`), which embeds the dependency list in the binary so image scanners can see the Rust crates:

```sh
cargo auditable build --release -p muxmultitool-server
```

### RPC contract and generated code

All frontend-backend calls are ConnectRPC. The contract lives in `crates/proto/proto/` and `buf` generates the Rust and TypeScript code from it. Generated code is not committed (`crates/proto/src/generated/` and `web/src/generated/` are git-ignored), so generate it before building, linting or testing either side, and again after every change to a `.proto` file.

One-time setup (`buf` must be on the PATH, see https://buf.build/docs/installation):

```sh
cargo install --locked protoc-gen-buffa protoc-gen-buffa-packaging connectrpc-codegen
(cd web && npm ci)   # installs protoc-gen-es
```

Regenerate:

```sh
cd crates/proto && buf generate
```

Contract checks (CI runs the same):

| Check | Command |
|---|---|
| Lint | `buf lint` in `crates/proto` |
| Format | `buf format -d --exit-code` in `crates/proto` |
| Breaking changes | `crates/proto/breaking.sh [base-branch]`, which runs `buf breaking` against `master` (skipped until `master` has the proto module) |

Methods that have no side effects must declare `option idempotency_level = NO_SIDE_EFFECTS;` so Connect may send them as `GET`. The `SystemService` in `crates/proto/proto/muxmultitool/system/v1/system.proto` is the example to copy.
### Running the server

`cargo run -p muxmultitool-server` starts the axum server on `127.0.0.1:8080`. It mounts the ConnectRPC services under `/rpc`, serves the built frontend (`web/dist`, from `npm run build`) at `/` and logs to stderr (`RUST_LOG` filters, default `info`). Every setting is a flag with an environment variable behind it, so a compose file needs no command line; `--help` lists them:

| Setting | Flag | Variable | Default |
|---|---|---|---|
| Listen address | `--listen` | `MMT_LISTEN` | `127.0.0.1` (use `0.0.0.0` in a container) |
| Port | `--port` | `MMT_PORT` | `8080` |
| Media folders | `--media-root`, repeatable | `MMT_MEDIA_ROOTS`, comma-separated | none |
| ffmpeg, ffprobe | `--ffmpeg`, `--ffprobe` | `MMT_FFMPEG`, `MMT_FFPROBE` | found on `PATH` |
| Frontend folder | `--frontend-dir` | `MMT_FRONTEND_DIR` | `web/dist` |
| Parallel jobs | `--jobs` | `MMT_JOBS` | `1` |
| Data folder | `--data-dir` | `MMT_DATA_DIR` | none: settings live in memory for this run |

Ctrl-C or `SIGTERM` (what `docker stop` sends) stops it cleanly. The health method is a plain JSON call, which also serves as the container `HEALTHCHECK`:

```sh
curl -X POST -H 'content-type: application/json' -d '{}' \
  http://127.0.0.1:8080/rpc/muxmultitool.system.v1.SystemService/GetHealth
# {"status":"HEALTH_STATUS_OK"}
```

The frontend sends JSON in development (readable in devtools) and binary protobuf in release builds; the server answers in whichever encoding a request uses.

### Media folders and file ids

The file browser (`FilesService` in `crates/proto/proto/muxmultitool/files/v1/files.proto`) only shows what is under the media folders: subfolders and files with a media extension, with hidden entries left out. `ListFolder` lists a folder (the media folders themselves when no id is given), `FindFiles` matches wildcards such as `*.mp4` against the file names in a folder, optionally in its subfolders too, and `GetEntries` looks ids up again.

Folders and files are named by opaque ids, which the client stores and passes back. The server resolves an id by canonicalizing the path (so symlinks are followed) and checking that the result is still under the media folder it belongs to; `..`, absolute paths and symlinks that lead outside are refused, and listings leave such entries out. The logic is `muxmultitool_core::files`. Ids stay valid across restarts as long as the media folders are configured in the same order.

Paths from the client are never accepted in the container. The desktop app runs the server with `Config::desktop` set, which turns on `OpenPaths`: it takes the absolute paths from native file dialogs and returns entries for them, so any file or folder the user picked can be browsed and used like one under a media folder.

### Stream report

`FilesService/GetStreamReport` is the per-stream report (the old `streaminfo.sh`): for every selected file, each stream's index, type, codec, language, title, flags and size, plus the container's format, duration and size. A call takes file ids and wildcard selections (`folder_id` + `patterns`, as `FindFiles` takes them) together and reports the files in that order, once each. A file ffprobe cannot read gets a plain-language error with ffprobe's output in its own report; the other files are unaffected. Only a bad selection (an unknown folder, an invalid pattern) fails the call. `GetStreamDetails` returns everything ffprobe reports about one stream, as flattened key/value fields in ffprobe's order (`disposition.default`, `tags.language`) and as the raw JSON object. The logic is `muxmultitool_core::report`.

ffprobe does not report stream sizes, so the server works them out and says how in each stream's `size_method`:

| Method | How | Exact? |
|---|---|---|
| `PACKETS` | The sizes of every packet of every stream, summed in one demux pass over the file (`ffprobe -show_entries packet=stream_index,size`, summed as it streams). Runs when a stream has no statistics tag, unless the request is `quick`. When it runs, every stream of the file takes its size from it. | Yes; costs one read of the file |
| `TAGS` | The Matroska statistics tag `NUMBER_OF_BYTES`, which mkvmerge writes and ffmpeg keeps on a stream copy. | As of when the tags were written; a file cut without rewriting them keeps the old numbers |
| `EXTRADATA` | Attachments (fonts, covers stored as attachments) have no packets; their size is that of the attached data. | Yes |
| `ESTIMATED` | Bit rate × duration ÷ 8 from ffprobe's per-stream values, for a `quick` report or when the packet pass failed. MP4 derives the bit rate from its sample table, so the estimate is close; Matroska and MPEG-TS often have no per-stream bit rate. | No |
| unspecified | None of the above applies; the size is 0 and means nothing. | — |

Sizes are returned in bytes and in decimal gigabytes (`size_gb`, 10^9 bytes). A `quick` report skips the packet pass, so it is as fast as reading the headers: use it to show the table at once and refine the sizes afterwards.

```sh
curl -X POST -H 'content-type: application/json' \
  -d '{"fileIds":["<id>"],"selections":[{"folderId":"<folder id>","patterns":["*.mkv"],"recursive":true}]}' \
  http://127.0.0.1:8080/rpc/muxmultitool.files.v1.FilesService/GetStreamReport
# {"files":[{"fileId":"…","entry":{…},"container":{"format":"matroska,webm",…},"streams":[{"type":"STREAM_TYPE_VIDEO","codec":"hevc","language":"und","sizeBytes":"21950000000","sizeGb":21.95,"sizeMethod":"SIZE_METHOD_PACKETS",…},…]}]}
```

### Jobs

Every operation that writes output runs as a job of the `JobsService` (`crates/proto/proto/muxmultitool/jobs/v1/jobs.proto`; the queue itself is `core::jobs`). Jobs live in the server process, so they keep running when the browser tab closes. `--jobs` sets how many run at once; the rest wait in submission order. The queue is in memory only: **queued and running jobs do not survive a server restart.** A stop (Ctrl-C, `docker stop`) cancels every job, kills its ffmpeg process and waits for it before the server exits, so no ffmpeg outlives the server.

Each job carries the exact ffmpeg command line it runs, known from submission on, and a failed job carries a plain-language message next to ffmpeg's own output. `WatchJob` follows one job and `WatchQueue` the whole queue: server streams that send a snapshot first and then every progress report, log line and state change. After a disconnect (a proxy timeout, a laptop that slept) the client calls again with the last event id it saw and gets the events it missed from a short per-job buffer, or a fresh snapshot when it fell too far behind. The frontend keeps one `WatchQueue` stream open for the whole app (`web/src/core/composables/useJobQueue.ts`, on the generic resumable runner in `web/src/core/libs/watchStream.ts`): it feeds the top-bar indicator, the Jobs screen and the job details, and reconnects with back-off, dropping any replayed event it already applied.

Jobs only read and write inside the media roots. A stream can be read with curl as well, using the Connect streaming envelope (a 5-byte header per message: flags, then the length):

```sh
printf '\0\0\0\0\2{}' | curl -sN -X POST -H 'content-type: application/connect+json' --data-binary @- \
  http://127.0.0.1:8080/rpc/muxmultitool.jobs.v1.JobsService/WatchQueue

### GIF

A time range of a video becomes an animated GIF, and a GIF becomes an MP4 (H.264) or WebM (VP9, or VP8 when the build has no VP9) video. Both are `JobsService` operations (`video_to_gif`, `gif_to_video`); the logic is `core::gif` and the messages are in `crates/proto/proto/muxmultitool/gif/v1/gif.proto`.

GIF output grows fast, so the user states **limits** rather than encoder settings: the longest range, the widest and tallest picture, and the largest file (10 s, 480 px, no height bound and 8 MB by default). The request's frame rate (1 to 50, 12 by default) and width (480 by default) are never more than the source's, the height follows the aspect ratio, and a quality of `SMALL`, `BALANCED` or `BEST` picks the palette (64, 128 or 256 colors) and the dither. The GIF is made in one pass with `palettegen` and `paletteuse` (`diff_mode=rectangle`, so static parts of the picture are not stored again).

`GifService/EstimateGif` runs the same plan as a submit without queueing anything: it returns the picture size, frame count, an **estimated** file size (frames × pixels × a per-quality byte factor, deliberately on the high side), the ffmpeg command, and every limit the GIF would break with what to lower, e.g. "the estimated size of 12.3 MB is over the 8 MB limit; shorten the range to 6 s or reduce the width to 360 px". A submit over a limit fails with `FAILED_PRECONDITION` and the same messages, so nothing runs. Both operations check the capability cache first and fail with `FAILED_PRECONDITION` when the build lacks the GIF encoder, the palette filters or the video encoder.

```sh
curl -X POST -H 'content-type: application/json' \
  -d '{"operation":{"input":"/media/trailer.mkv","startMs":12000,"endMs":20000,"fps":12,"width":480,"limits":{"maxBytes":"4000000"}}}' \
  http://127.0.0.1:8080/rpc/muxmultitool.gif.v1.GifService/EstimateGif
# {"estimate":{"width":480,"height":200,"fps":12,"durationMs":"8000","frames":96,"estimatedBytes":"4149728"},"violations":[{"limit":"GIF_LIMIT_SIZE","message":"the estimated size of 4.1 MB is over the 4 MB limit; shorten the range to 7 s or reduce the width to 470 px","suggestedDurationMs":"7000","suggestedWidth":470}],"command":"…"}
```

### Streams: remux and selective copy

A remux (the whole file into another container) and a selective stream copy (tick streams to keep or drop, down to a video-only file) are one `JobsService` operation, `select_streams`; the logic is `core::streams` and the messages are in `crates/proto/proto/muxmultitool/streams/v1/streams.proto`. Nothing is re-encoded: the command is the input, one `-map 0:<index>` per kept stream (`-map 0` when every stream is kept), `-c copy` and the output. The output's extension picks the container (`.mkv`/`.mka`/`.mks`, `.mp4`/`.m4v`/`.m4a`, `.mov`, `.webm`); the output is an absolute path inside a media root or a bare file name, which puts it next to the input. The input is a file id from `FilesService` or the absolute path of a file inside a media root.

A selection has a `mode`: `ALL` (a remux), `KEEP` or `DROP` with `stream_indexes`, or `VIDEO_ONLY`, which drops audio, subtitles, attachments, data and cover art. `StreamsService/PreviewStreams` resolves it without queueing anything: every stream of the input marked kept or dropped with the reason (the report's "why" column), the exact ffmpeg command, and the **issues**: kept streams the container is known to refuse on a copy (SubRip or ASS subtitles, attachments and TrueHD in MP4; Opus and FLAC in MOV; anything but VP8/VP9/AV1, Vorbis/Opus and WebVTT in WebM; `mov_text` subtitles and data streams in MKV), each with what to do, e.g. "MP4 cannot hold subrip subtitles (stream 2); drop stream 2 or choose MKV". A codec the table does not know gets a non-blocking warning instead. A submit with a blocking issue fails with `FAILED_PRECONDITION` and the same messages, so the job never starts; the table is `core::streams::containers`.

```sh
curl -X POST -H 'content-type: application/json' \
  -d '{"operation":{"input":"/media/movie.mkv","output":"movie.eng.mp4","mode":"SELECTION_MODE_KEEP","streamIndexes":[0,1]}}' \
  http://127.0.0.1:8080/rpc/muxmultitool.streams.v1.StreamsService/PreviewStreams
# {"container":"MP4","outputName":"movie.eng.mp4","streams":[{"stream":{"index":0,"type":"STREAM_TYPE_VIDEO","codec":"hevc",…},"kept":true,"why":"ticked"},…],"canRun":true,"command":"… -i /media/movie.mkv -map 0:0 -map 0:1 -c copy /media/movie.eng.mp4","argv":[…]}
```

### Cutting

Lossless cutting is `CutService` (`crates/proto/proto/muxmultitool/cut/v1/cut.proto`) plus the `cut` operation of `JobsService`; the logic is `core::cut`. The user marks segments of one file in seconds, keeps or removes them, and exports the kept ranges as one file each or joined into one, every stream copied. Segments are normalised first: clamped to the file, sorted, overlapping ones merged, empty ones dropped (touching ones stay separate, so a split into parts works). `PlanCut` shows what a `CutOperation` would do without queueing anything: the kept ranges, where each cut really starts, the output names, the exact ffmpeg command and, for a merged export, the concat list. `GetKeyframes` lists the keyframes of a time range for the timeline, and `ParseTime` turns typed times (`754.5`, `12:34`, `0:12:34,567`, `1h2m3.5s`, `0:12:34:12` frames) into seconds, checked against the file's duration.

A packet of video can only be decoded from a keyframe on, so the **cut mode** decides where a range starts; where it ends is free, within a frame or two:

| Mode | Start of the range | Output length | Use |
|---|---|---|---|
| **Keyframe cut** | The last keyframe at or before the start time, for every stream. Nothing after the start is lost; up to one GOP is added in front. | The range plus that lead-in | The default: clean playback everywhere |
| **Normal cut** | The start time itself: audio, subtitles and data begin exactly there; the video joins at the next keyframe, so players show no picture (black, or the first frame held) for up to one GOP. Nothing before the start is included. | The range | When nothing before the start may be kept |

Smart cut (re-encoding only the frames up to the next keyframe) is a later task; it builds on the same plan.

How the commands are built (measured on ffmpeg 8, see the `core::cut` docs): an input `-ss` placed a second before the cut, so the demuxer lands before it whatever the container does (MP4 and Matroska land on a keyframe, one too early when the target sits exactly on one; MPEG-TS lands wherever its binary search ends), then `-copyts` and an output `-ss`/`-t`, which ffmpeg applies by decode time: at the keyframe's decode time for a keyframe cut, so the keyframe is kept and the audio leads it by at most the reorder delay, at the start time for a normal cut. Separate files are one ffmpeg run with one input per range. A merged export feeds the ranges to the `concat` demuxer as `inpoint`/`outpoint`/`duration` entries, with the list inside the command as a `data:` URL (no scratch file); the inpoint is nudged 50 ms past the keyframe so the demuxer's seek lands on it. Junctions of a merged file may carry a few overlapping audio packets, which ffmpeg re-stamps (a short hiccup, as with any lossless join). Merged exports need a video stream and a container with a keyframe index: audio-only files, MPEG transport and program streams are refused with `FAILED_PRECONDITION` and go to separate files instead. A FLAC cut keeps the source's `STREAMINFO` header, so ffprobe reports the old length for it; the audio is cut correctly.

For separate files the `output` is a template: `{n}` is the range's number (zero-padded), `{start}` and `{end}` its times as `HH.MM.SS.mmm`; a name without a placeholder gets `-{n}` before its extension, and one without an extension takes the input's. Outputs must be new files in an existing folder inside a media root. The `input` is a file id from `FilesService` or an absolute path inside a media root.

```sh
curl -X POST -H 'content-type: application/json' \
  -d '{"operation":{"input":"/media/movie.mkv","segments":[{"startSeconds":648,"endSeconds":716.2},{"startSeconds":732.5,"endSeconds":790}],"cutMode":"CUT_MODE_KEYFRAME","export":"CUT_EXPORT_SEPARATE_FILES","output":"/media/out/movie-{n}.mkv"}}' \
  http://127.0.0.1:8080/rpc/muxmultitool.cut.v1.CutService/PlanCut
# {"segments":[{"number":1,"startSeconds":648,"endSeconds":716.2,"cutStartSeconds":646.04,"output":"/media/out/movie-1.mkv"},…],"outputs":[…],"command":"… -ss 645.04 -i /media/movie.mkv -ss 729.5 -i /media/movie.mkv -copyts -map 0 -ss 645.959 -t 70.241 -c copy -avoid_negative_ts make_zero /media/out/movie-1.mkv …","totalDurationSeconds":129.66}
```

### Preview (HLS)

The player never gets a source file: `MediaService/StartPreview` starts a preview session for a file and returns the playlist path, `/media/{id}/hls/index.m3u8`, which hls.js plays. Segments (`/media/{id}/hls/{n}.ts`) are H.264 + AAC in MPEG-TS, produced on demand by ffmpeg from wherever the player seeks: copied when the source already is H.264 (8-bit 4:2:0) and AAC, transcoded with x264 otherwise (hardware encoders come after the MVP). Exports always read the original file.

- Transcoded segments are 2 s on a fixed grid; copied segments start at the source's keyframes (at least 2 s, at most 30 s apart, else the video is transcoded). The playlist is fixed when the session starts.
- Seek target: playback resumes within about 3 s after a seek on a typical machine (a 1080p source; 4K sources are scaled to 1080p). Measured here with a 720p HEVC/DTS MKV in headless Chromium: first frame after 1.0 s, a seek into unencoded video resumed after 0.55 s.
- ffmpeg stops after 30 s without segment requests and restarts where the player continues; a session and its segments are removed after 30 min without requests, on `StopPreview`, or when the server stops. Segments live under the system temporary folder (`TMPDIR`), in a `muxmultitool-preview-*` folder with a random name, made with the first session.
- Until `FilesService` issues file ids, `file_id` is the absolute path of a file inside a media root (`--media-root`).

```sh
curl -X POST -H 'content-type: application/json' -d '{"fileId":"/media/movie.mkv"}' \
  http://127.0.0.1:8080/rpc/muxmultitool.media.v1.MediaService/StartPreview
# {"mediaId":"…","playlistUrl":"/media/…/hls/index.m3u8","durationSeconds":5400.0,"mode":"PREVIEW_MODE_TRANSCODE","segmentCount":2700}
```

### Timeline: thumbnails, waveform, subtitle cues

The cut editor's timeline is fed by three background analyses in `MediaService`, each a server stream that sends what is known at once and then every addition until the analysis ends (`muxmultitool_core::timeline`). Results are cached per file and reused until the file changes (size or modification time); a changed file starts over under a new id.

- `GetThumbnails` plans a sprite sheet (160 px wide tiles, 10 × 10 per sprite, at most 1200 tiles, at least 0.5 s apart) and generates it with one ffmpeg process. The sprites are images the browser fetches by URL, `/media/{id}/thumbs/{n}.jpg`, cacheable for a day. The response's `sheet` maps a time to a tile; files with more than 36 000 frames are decoded keyframes-only (`keyframes_only`), so a tile shows the last keyframe before its time.
- `GetWaveform` returns the peaks of one audio stream for a time window at a requested resolution (at most 8192 peaks), as `mins` and `maxs` in -1..1. ffmpeg measures 50 peaks per second once per stream (`astats`, reported on stderr, nothing on disk); every window is a downsample of that.
- `GetSubtitleCues` returns the cues of a text subtitle stream as plain text (tags and positioning dropped), converted by ffmpeg to WebVTT. Image-based subtitles (PGS, VobSub, DVB) are refused with `FAILED_PRECONDITION`.
- `file_id` is an id from `FilesService`, or the absolute-path form `StartPreview` takes. Sprites live under a `muxmultitool-timeline-*` folder with a random name in the system temporary folder, made with the first file; at most 16 files are cached, the least recently used is dropped beyond that, and everything is removed when the server stops.
- All three are server streams, so `curl` needs the Connect streaming envelope described under "Jobs" above.

### Editor setup and git hooks

`.editorconfig` sets tabs as the baseline (YAML and `.proto` use two spaces). `.vscode/` makes the oxc extension the formatter for `.ts` and `.vue`, turns off Vue (Official)'s formatter and sets tab indentation; accept the recommended extensions when VS Code asks.

Pre-commit hooks run the fast format and lint checks through [lefthook](https://lefthook.dev) (`lefthook.yml`). Install them once per clone:

```sh
cd web && npm ci && npx lefthook install
```

The hooks also need `typos` (`cargo install typos-cli --locked`), `committed` (`cargo install committed --locked`) and, for Rust files, the toolchain from `rust-toolchain.toml`. Run `typos` by hand to check spelling across the repo.

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org) (`feat(core): add the job queue`), because the release version is worked out from them. The rules are in `committed.toml`; the commit-msg hook and CI check them.

### CI

GitHub Actions runs the checks above on every pull request and on `master` (`.github/workflows/ci.yml`). The `changes` job looks at the changed paths and skips what they don't touch:

| Changed | Runs |
|---|---|
| `crates/core/**`, `crates/proto/**`, `Cargo.lock`, `Cargo.toml`, `rust-toolchain.toml`, `.github/**` | Everything |
| `crates/server/**`, `crates/desktop/**`, `Dockerfile`, `tauri.conf.json`, the root `*.toml` configs | Backend checks |
| `web/**` | Frontend checks |
| `crates/proto/**` | RPC contract checks (lint, format, breaking changes) |
| anything else (readme, docs) | Nothing |

The jobs: `codegen` runs `buf generate` once and hands the result to the others; `backend-lint`, `backend-test` (nextest with coverage, needs ffmpeg) and `backend-musl` (`cargo zigbuild` of the server for `x86_64-unknown-linux-musl`, so a C dependency can't slip in); `frontend` runs the `npm run ci` steps one by one, then the unit tests; `contract` runs the buf checks; `commits` runs `committed` over the new commits. `CI` is the one status check to require in branch protection: it is green when every job passed or was skipped. Cargo builds are cached with `Swatinem/rust-cache`, npm with `actions/setup-node`.

A daily workflow (`.github/workflows/security.yml`) runs `cargo deny check advisories` and `npm audit`.

Without a GitHub remote, check the workflows with [actionlint](https://github.com/rhysd/actionlint) and run the steps' commands locally.

### Settings

`SystemService/GetSettings` and `UpdateSettings` keep the user settings (the colour theme) on the server, so every browser tab and the desktop window see the same choice. The store is `core::settings`: a `Settings` value, written as `settings.json` in `--data-dir` when one is configured (the container mounts a volume there; the desktop app passes its app-data folder), kept in memory only otherwise. The frontend also remembers the theme in the browser, so the choice survives a page reload even without a data folder.

### Frontend

The Vue app lives in `web/`. From there: `npm ci`, `npm run dev`, and `npm run ci` for the format, lint, type-check and build steps.

Themes are palette files in `web/src/core/themes/families/`, one per family, registered in `web/src/core/themes/index.ts`. A palette gives each variant its official colours; the semantic `--mmt-*` tokens that every component uses through Tailwind are derived from them at runtime, nudged just far enough for WCAG AA contrast. `npx vitest run src/core/themes` checks every variant.

## License

MuxMultiTool is licensed under the [GNU General Public License v3.0 or later](https://www.gnu.org/licenses/gpl-3.0.html).

SPDX-License-Identifier: GPL-3.0-or-later
