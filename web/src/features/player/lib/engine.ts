/**
 * Which playback engine plays the HLS preview, as decided by the TASK-4 spike (architecture.md, "HLS
 * playback spike"): hls.js on MediaSource wherever MSE can decode H.264/AAC, which covers Chromium,
 * Firefox, WebView2, CEF and desktop WKWebView; the browser's own HLS player only where MSE is absent.
 * `canPlayType` alone is never trusted, because Chromium answers "maybe" with a player that differs per
 * version.
 */

export type PlaybackEngine = "hls.js" | "native" | "none";

/** What the preview contains: H.264 Main with AAC-LC; what the spike checked. */
export const PREVIEW_MSE_TYPE = 'video/mp4; codecs="avc1.4d401e,mp4a.40.2"';
export const HLS_MIME_TYPE = "application/vnd.apple.mpegurl";

export interface PlaybackEnvironment {
	/** `window.MediaSource`, or `undefined` where there is none. */
	mediaSource?: { isTypeSupported(type: string): boolean };
	/** `video.canPlayType`. */
	canPlayType: (type: string) => string;
}

/** Picks the engine for an environment. Pure, so the rule can be tested without a browser. */
export function choosePlaybackEngine(env: PlaybackEnvironment): PlaybackEngine {
	if (env.mediaSource?.isTypeSupported(PREVIEW_MSE_TYPE)) {
		return "hls.js";
	}
	if (env.canPlayType(HLS_MIME_TYPE) !== "") {
		return "native";
	}
	return "none";
}

/** The engine for this browser and video element. */
export function detectPlaybackEngine(video: HTMLVideoElement): PlaybackEngine {
	const mediaSource = (globalThis as { MediaSource?: PlaybackEnvironment["mediaSource"] }).MediaSource;
	return choosePlaybackEngine({ mediaSource, canPlayType: (type) => video.canPlayType(type) });
}

export interface AttachedSource {
	/** The engine in use. */
	engine: PlaybackEngine;
	/** The hls.js version when that engine is used. */
	version?: string;
	/** Detaches the source and frees the engine. */
	detach(): void;
}

export interface AttachOptions {
	/** Called for a fatal error, after which playback is dead until the source is attached again. */
	onFatalError?: (message: string) => void;
}

/**
 * Attaches the playlist at `url` to `video` with `engine`. hls.js is loaded on first use, so pages
 * without a player never download it.
 */
export async function attachPreview(
	video: HTMLVideoElement,
	url: string,
	engine: PlaybackEngine,
	options: AttachOptions = {}
): Promise<AttachedSource> {
	if (engine === "native") {
		video.src = url;
		return {
			engine,
			detach: () => {
				video.removeAttribute("src");
				video.load();
			}
		};
	}
	if (engine === "none") {
		return { engine, detach: () => {} };
	}
	// The light build: no alternate audio, subtitles, EME, CMCD or interstitials; the preview is one
	// H.264 + AAC rendition and the cues come from the stream report
	const { default: Hls } = await import("hls.js/light");
	const hls = new Hls({
		// The preview is a VOD playlist; nothing is live
		lowLatencyMode: false,
		// Segments are 2 s; keep a modest window so seeks inside it are instant and memory stays small
		maxBufferLength: 30,
		backBufferLength: 30,
		// Fail fast on a segment the server could not produce instead of retrying for a minute
		fragLoadPolicy: {
			default: {
				maxTimeToFirstByteMs: 60_000,
				maxLoadTimeMs: 120_000,
				timeoutRetry: { maxNumRetry: 2, retryDelayMs: 1000, maxRetryDelayMs: 4000 },
				errorRetry: { maxNumRetry: 2, retryDelayMs: 1000, maxRetryDelayMs: 4000 }
			}
		}
	});
	hls.on(Hls.Events.ERROR, (_event, data) => {
		if (data.fatal) {
			options.onFatalError?.(`${data.type}/${data.details}`);
		}
	});
	hls.loadSource(url);
	hls.attachMedia(video);
	return {
		engine,
		version: Hls.version,
		detach: () => hls.destroy()
	};
}
