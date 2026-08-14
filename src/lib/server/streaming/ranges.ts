import { extname } from 'node:path';
import { stat, open } from 'node:fs/promises';

const BLOCK_SIZE = 64 * 1024;

const MEDIA_TYPES: Record<string, string> = {
	'.mp3': 'audio/mpeg',
	'.m4a': 'audio/mp4',
	'.m4b': 'audio/mp4',
	'.flac': 'audio/flac',
	'.ogg': 'audio/ogg',
	'.opus': 'audio/ogg',
	'.wav': 'audio/wav',
	'.aac': 'audio/aac'
};

export class RangeNotSatisfiable extends Error {
	constructor(public size: number) {
		super('Range Not Satisfiable');
	}
}

export function mediaType(path: string): string {
	return MEDIA_TYPES[extname(path).toLowerCase()] ?? 'application/octet-stream';
}

/** Parses `Range: bytes=<start>-<end>`. Single range only (ignores anything
 * after a comma). Returns null for a missing or unparseable header (caller
 * should then serve the whole file with 200). Throws RangeNotSatisfiable for
 * a range whose start is at/beyond size, or whose end precedes its start, or
 * a non-positive suffix length. */
export function parseRange(
	header: string | null,
	size: number
): { start: number; end: number } | null {
	if (!header || !header.startsWith('bytes=')) return null;
	const part = header.slice('bytes='.length).split(',')[0].trim();
	const sepIndex = part.indexOf('-');
	if (sepIndex === -1) return null;
	const startText = part.slice(0, sepIndex);
	const endText = part.slice(sepIndex + 1);

	if (startText === '') {
		if (!/^\d+$/.test(endText)) return null;
		const length = Number(endText);
		if (length <= 0) throw new RangeNotSatisfiable(size);
		return { start: Math.max(0, size - length), end: size - 1 };
	}

	if (!/^\d+$/.test(startText)) return null;
	const start = Number(startText);
	if (start >= size) throw new RangeNotSatisfiable(size);
	let end = /^\d+$/.test(endText) ? Number(endText) : size - 1;
	end = Math.min(end, size - 1);
	if (end < start) throw new RangeNotSatisfiable(size);
	return { start, end };
}

async function readSlice(
	path: string,
	start: number,
	end: number
): Promise<ReadableStream<Uint8Array>> {
	const handle = await open(path, 'r');
	let position = start;
	const total = end - start + 1;
	let sent = 0;
	return new ReadableStream({
		async pull(controller) {
			const remaining = total - sent;
			if (remaining <= 0) {
				await handle.close();
				controller.close();
				return;
			}
			const size = Math.min(BLOCK_SIZE, remaining);
			const buffer = Buffer.alloc(size);
			const { bytesRead } = await handle.read(buffer, 0, size, position);
			if (bytesRead === 0) {
				await handle.close();
				controller.close();
				return;
			}
			position += bytesRead;
			sent += bytesRead;
			controller.enqueue(buffer.subarray(0, bytesRead));
		},
		async cancel() {
			await handle.close();
		}
	});
}

async function readWhole(path: string): Promise<ReadableStream<Uint8Array>> {
	const handle = await open(path, 'r');
	return new ReadableStream({
		async pull(controller) {
			const buffer = Buffer.alloc(BLOCK_SIZE);
			const { bytesRead } = await handle.read(buffer, 0, BLOCK_SIZE, null);
			if (bytesRead === 0) {
				await handle.close();
				controller.close();
				return;
			}
			controller.enqueue(buffer.subarray(0, bytesRead));
		},
		async cancel() {
			await handle.close();
		}
	});
}

/** Exact port of streaming/ranges.py::range_response. `head=true` returns
 * identical headers/status with no body and — critically — never opens the
 * file, matching the perf reasoning in the Python docstring (a HEAD probe
 * must not pay to read the file just to have the body discarded). */
export async function rangeResponse(
	path: string,
	rangeHeader: string | null,
	head: boolean
): Promise<Response> {
	const stats = await stat(path);
	const size = stats.size;
	const contentType = mediaType(path);

	if (rangeHeader === null) {
		const headers = {
			'content-type': contentType,
			'content-length': String(size),
			'accept-ranges': 'bytes'
		};
		if (head) return new Response(null, { status: 200, headers });
		return new Response(await readWhole(path), { status: 200, headers });
	}

	let span: { start: number; end: number } | null;
	try {
		span = parseRange(rangeHeader, size);
	} catch (e) {
		if (e instanceof RangeNotSatisfiable) {
			return new Response(null, { status: 416, headers: { 'content-range': `bytes */${size}` } });
		}
		throw e;
	}

	if (span === null) {
		// Header present but not understood — RFC 9110 permits ignoring it and
		// serving the whole file with 200 (self-built, not delegated).
		const headers = {
			'content-type': contentType,
			'content-length': String(size),
			'accept-ranges': 'bytes'
		};
		if (head) return new Response(null, { status: 200, headers });
		return new Response(await readWhole(path), { status: 200, headers });
	}

	const { start, end } = span;
	const headers = {
		'content-type': contentType,
		'content-range': `bytes ${start}-${end}/${size}`,
		'content-length': String(end - start + 1),
		'accept-ranges': 'bytes'
	};
	if (head) return new Response(null, { status: 206, headers });
	return new Response(await readSlice(path, start, end), { status: 206, headers });
}
