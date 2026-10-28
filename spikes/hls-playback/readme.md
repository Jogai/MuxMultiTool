# HLS playback spike (TASK-4)

Disposable prototype, not part of the product. It answers one question: does an
H.264/AAC HLS stream served from `127.0.0.1` play and seek in each webview the
app runs in? The result is recorded in `docs/architecture.md`, section "Desktop
specifics"; this folder only exists to repeat the test.

## Files

| File | Purpose |
|---|---|
| `make-clip.sh` | Writes a 60 s test clip (testsrc2 + 440 Hz sine, H.264 Main + AAC-LC) as a VOD HLS playlist with 4 s MPEG-TS segments into `clip/`. |
| `serve.mjs` | Static server on `127.0.0.1` for the page, hls.js and `clip/` (served under `/hls/`). |
| `index.html` | The page: a `<video>` with play/seek/mute buttons and an event log. Picks native HLS where the browser claims it, hls.js on MSE otherwise. `?engine=native` or `?engine=hlsjs` forces one. State is on `window.spike`. |
| `test.mjs` | Playwright driver for headless Chromium and Firefox: plays unmuted, seeks forward, backward and near the end, and checks that frames keep decoding after each seek. Screenshots and `results.json` go to `out/`. |

## Run

```sh
npm install
npx playwright install chromium firefox   # once; browsers go to ~/.cache/ms-playwright
npm run clip
npm test                                  # or: node test.mjs chromium
npm run serve                             # then open http://127.0.0.1:8787/ by hand
```

Headless notes (Debian 13 container, no root):
- Playwright's Firefox links GTK 3. If the host lacks it, download the `.deb` files with a user-local apt state (`apt-get -o Dir::State=... -o Dir::Cache=... install --download-only libgtk-3-0t64 libxcursor1`), extract them with `dpkg-deb -x` and put the `usr/lib/x86_64-linux-gnu` folder on `LD_LIBRARY_PATH`.
- Firefox on Linux decodes H.264 and AAC through the system `libavcodec`, not its bundled `libmozavcodec`. Without it `MediaSource.isTypeSupported('video/mp4; codecs="avc1.4d401e,mp4a.40.2"')` is `false` and the test reports "no playback path". Add `libavcodec61` (and its dependencies) to the same extracted folder.

## Manual test in a desktop shell

For WebView2 (Windows), WKWebView (macOS) and the Tauri v3 alpha CEF runtime
(Linux), which can't be driven from this container:

1. `npm run clip && npm run serve` on the machine under test.
2. Make a minimal Tauri app with the pinned alpha (`tauri`, `tauri-runtime-wry` or `tauri-runtime-cef`, `@tauri-apps/cli`) whose window loads `http://127.0.0.1:8787/`. Record the exact versions.
3. Open the page with `?engine=hlsjs`, press Play, then each seek button. Check: the clock in the picture jumps to the target, the tone keeps playing, the status returns to `playing`, the log shows no `hls.js fatal` or `media error` lines.
4. Repeat with `?engine=native`. On WKWebView this uses Apple's HLS stack. On WebView2 and CEF the engine line should read `none` or playback should fail quickly; either outcome is fine, the point is to know.
5. Also try: a seek while the stream is still buffering, pause/resume, mute toggle, and leaving it playing to the end.
6. Note the Tauri version, the webview version (reported in the user agent) and every oddity in `docs/architecture.md`.
