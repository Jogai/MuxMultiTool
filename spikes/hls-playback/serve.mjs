// Tiny static server for the spike. Serves the page, hls.js and the HLS clip
// from 127.0.0.1, which is also how the desktop app serves its preview stream.
//
//   node serve.mjs [port]      default port 8787; port 0 picks a free one
//
// Prints "listening http://127.0.0.1:<port>/" once it's up, so test.mjs can
// read the chosen port from stdout.
import { createServer } from 'node:http';
import { createReadStream } from 'node:fs';
import { stat } from 'node:fs/promises';
import { dirname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const port = Number(process.argv[2] ?? process.env.PORT ?? 8787);

const types = {
	'.html': 'text/html; charset=utf-8',
	'.js': 'text/javascript; charset=utf-8',
	'.m3u8': 'application/vnd.apple.mpegurl',
	'.ts': 'video/mp2t',
};

function resolve(url) {
	const path = new URL(url, 'http://localhost').pathname;
	if (path === '/') return join(root, 'index.html');
	if (path === '/hls.js') return join(root, 'node_modules/hls.js/dist/hls.min.js');
	if (path.startsWith('/hls/')) {
		const rel = normalize(path.slice('/hls/'.length));
		if (rel.startsWith('..')) return null;
		return join(root, 'clip', rel);
	}
	return null;
}

const server = createServer(async (req, res) => {
	const file = resolve(req.url);
	let info;
	try {
		info = file && await stat(file);
	} catch {
		info = null;
	}
	if (!info?.isFile()) {
		res.writeHead(404, { 'content-type': 'text/plain' }).end('not found');
		return;
	}
	const ext = file.slice(file.lastIndexOf('.'));
	res.writeHead(200, {
		'content-type': types[ext] ?? 'application/octet-stream',
		'content-length': info.size,
		// Segments of a VOD stream never change; the playlist is re-read on seek.
		'cache-control': ext === '.ts' ? 'public, max-age=3600' : 'no-cache',
	});
	if (req.method === 'HEAD') {
		res.end();
		return;
	}
	createReadStream(file).pipe(res);
});

server.listen(port, '127.0.0.1', () => {
	console.log(`listening http://127.0.0.1:${server.address().port}/`);
});
