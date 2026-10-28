// Drives the spike page headless in Chromium and Firefox: starts serve.mjs,
// waits for playback, seeks forward and backward, and checks that the clip
// keeps playing and decoding frames after each seek. Screenshots and a JSON
// result land in out/.
//
//   npx playwright install chromium firefox   (once)
//   npm run clip && npm test
import { spawn } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium, firefox } from 'playwright';

const root = dirname(fileURLToPath(import.meta.url));
const out = join(root, 'out');
await mkdir(out, { recursive: true });

function startServer() {
	return new Promise((resolve, reject) => {
		const child = spawn(process.execPath, [join(root, 'serve.mjs'), '0'], { stdio: ['ignore', 'pipe', 'inherit'] });
		child.stdout.on('data', (chunk) => {
			const match = /listening (http:\/\/[^/]+\/)/.exec(String(chunk));
			if (match) resolve({ child, url: match[1] });
		});
		child.on('exit', (code) => reject(new Error(`server exited with ${code}`)));
	});
}

async function waitFor(page, label, predicate, timeout = 15000, arg = null) {
	try {
		await page.waitForFunction(predicate, arg, { timeout });
	} catch (error) {
		const state = await page.evaluate(() => ({
			time: document.getElementById('video').currentTime,
			readyState: document.getElementById('video').readyState,
			log: window.spike.log.slice(-8),
			errors: window.spike.errors,
		}));
		throw new Error(`${label}: timed out after ${timeout} ms; state ${JSON.stringify(state)}`, { cause: error });
	}
}

// Which engines the page would accept in this browser, before playing anything.
async function probeEngines(browserType, launchOptions, url) {
	const browser = await browserType.launch({ headless: true, ...launchOptions });
	try {
		const page = await browser.newPage();
		await page.goto(url, { waitUntil: 'load' });
		return await page.evaluate(() => {
			const v = document.getElementById('video');
			return {
				version: navigator.userAgent,
				nativeHls: v.canPlayType('application/vnd.apple.mpegurl'),
				mse: typeof MediaSource !== 'undefined' && MediaSource.isTypeSupported('video/mp4; codecs="avc1.4d401e,mp4a.40.2"'),
				mseTs: typeof MediaSource !== 'undefined' && MediaSource.isTypeSupported('video/mp2t; codecs="avc1.4d401e,mp4a.40.2"'),
			};
		});
	} finally {
		await browser.close();
	}
}

async function runBrowser(name, browserType, launchOptions, url, engine) {
	const result = { browser: name, engine, ok: false, steps: [], errors: [] };
	const browser = await browserType.launch({ headless: true, ...launchOptions });
	result.version = browser.version();
	const page = await browser.newPage();
	page.on('console', (msg) => { if (msg.type() === 'error') result.errors.push(`console: ${msg.text()}`); });
	try {
		await page.goto(`${url}?engine=${engine}`, { waitUntil: 'load' });
		await waitFor(page, 'engine selection', () => window.spike.ready);
		const chosen = await page.evaluate(() => window.spike.engine);
		if (chosen !== { native: 'native', hlsjs: 'hls.js' }[engine]) throw new Error(`page chose ${chosen}`);
		result.steps.push(`engine ${chosen}`);

		// Play unmuted so AAC is decoded too; the launch options allow autoplay.
		await page.evaluate(() => document.getElementById('video').play());
		await waitFor(page, 'initial playback', () => {
			const v = document.getElementById('video');
			return v.currentTime > 1.5 && v.readyState >= 3 && v.getVideoPlaybackQuality().totalVideoFrames > 10;
		});
		const start = await page.evaluate(() => {
			const v = document.getElementById('video');
			return { time: v.currentTime, size: `${v.videoWidth}x${v.videoHeight}`, frames: v.getVideoPlaybackQuality().totalVideoFrames, dropped: v.getVideoPlaybackQuality().droppedVideoFrames };
		});
		result.steps.push(`playing from start: ${JSON.stringify(start)}`);
		if (start.size !== '640x360') throw new Error(`unexpected video size ${start.size}`);
		await page.screenshot({ path: join(out, `${name}-start.png`) });

		for (const target of [40, 10, 57]) {
			const before = await page.evaluate(() => document.getElementById('video').getVideoPlaybackQuality().totalVideoFrames);
			await page.evaluate((t) => { document.getElementById('video').currentTime = t; }, target);
			await waitFor(page, `seek to ${target}`, (t) => {
				const v = document.getElementById('video');
				return !v.seeking && v.currentTime >= t && v.currentTime < t + 1.5 && !v.paused && v.readyState >= 3;
			}, 15000, target);
			await page.waitForTimeout(1200);
			const after = await page.evaluate(() => {
				const v = document.getElementById('video');
				return { time: v.currentTime, frames: v.getVideoPlaybackQuality().totalVideoFrames, paused: v.paused };
			});
			if (after.frames <= before) throw new Error(`no new frames decoded after seek to ${target}: ${JSON.stringify(after)}`);
			if (after.time < target + 0.5) throw new Error(`playback did not advance after seek to ${target}: ${JSON.stringify(after)}`);
			result.steps.push(`seek to ${target}: ${JSON.stringify(after)}`);
			await page.screenshot({ path: join(out, `${name}-seek${target}.png`) });
		}

		await waitFor(page, 'end of clip', () => document.getElementById('video').ended, 10000);
		result.steps.push('reached end of clip');

		const spike = await page.evaluate(() => window.spike);
		result.errors.push(...spike.errors);
		result.log = spike.log;
		result.ok = spike.errors.length === 0;
	} catch (error) {
		result.errors.push(String(error.message));
		await page.screenshot({ path: join(out, `${name}-failure.png`) }).catch(() => {});
	} finally {
		await browser.close();
	}
	return result;
}

const browsers = {
	chromium: [chromium, { args: ['--autoplay-policy=no-user-gesture-required'] }],
	firefox: [firefox, { firefoxUserPrefs: { 'media.autoplay.default': 0, 'media.autoplay.blocking_policy': 0 } }],
};
// `node test.mjs firefox` runs one browser; no argument runs both.
const selected = process.argv.slice(2).length ? process.argv.slice(2) : Object.keys(browsers);

const { child: server, url } = await startServer();
const results = [];
try {
	for (const name of selected) {
		const [browserType, launchOptions] = browsers[name];
		const probe = await probeEngines(browserType, launchOptions, url);
		console.log(`${name}: canPlayType(HLS)="${probe.nativeHls}" MSE fmp4=${probe.mse} MSE ts=${probe.mseTs}`);
		// Native HLS is tried whenever the browser claims it; hls.js whenever MSE
		// accepts H.264/AAC. A browser with neither (Firefox without a system
		// libavcodec, for instance) fails with the probe as the explanation.
		const engines = [probe.nativeHls !== '' && 'native', probe.mse && 'hlsjs'].filter(Boolean);
		if (!engines.length) {
			results.push({ browser: name, engine: null, ok: false, steps: [], errors: ['no playback path: no native HLS and MSE rejects H.264/AAC'], probe });
			continue;
		}
		for (const engine of engines) {
			const result = await runBrowser(name, browserType, launchOptions, url, engine);
			result.probe = probe;
			results.push(result);
		}
	}
} finally {
	server.kill();
}

await writeFile(join(out, 'results.json'), JSON.stringify(results, null, '\t') + '\n');
for (const r of results) {
	console.log(`${r.ok ? "PASS" : "FAIL"} ${r.browser} ${r.version} engine=${r.engine}`);
	for (const step of r.steps) console.log(`  ${step}`);
	for (const err of r.errors) console.log(`  error: ${err}`);
}
process.exit(results.every((r) => r.ok) ? 0 : 1);
