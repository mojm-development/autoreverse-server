// Pixel diff for the design comparison (compare.sh), dependency-free: decodes the 8-bit,
// non-interlaced PNGs Playwright writes, compares each <shot>__legacy.png with its __rust.png and
// writes <shot>__diff.png (legacy dimmed, differing pixels red, size overhang magenta) plus
// report.txt sorted by the share of differing pixels.
//
//   node e2e/design/diff.mjs <dir>
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { deflateSync, inflateSync } from 'node:zlib';

const dir = process.argv[2];
if (!dir) throw new Error('usage: diff.mjs <dir>');

function decode(file) {
	const buf = readFileSync(file);
	let pos = 8;
	let width = 0;
	let height = 0;
	let colorType = 0;
	const idat = [];
	while (pos < buf.length) {
		const len = buf.readUInt32BE(pos);
		const type = buf.toString('ascii', pos + 4, pos + 8);
		const data = buf.subarray(pos + 8, pos + 8 + len);
		if (type === 'IHDR') {
			width = data.readUInt32BE(0);
			height = data.readUInt32BE(4);
			if (data[8] !== 8 || data[12] !== 0) throw new Error(`${file}: nur 8 bit, ohne Interlace`);
			colorType = data[9];
		} else if (type === 'IDAT') idat.push(data);
		pos += 12 + len;
	}
	const bpp = { 2: 3, 6: 4, 0: 1, 4: 2 }[colorType];
	const raw = inflateSync(Buffer.concat(idat));
	const stride = width * bpp;
	const out = Buffer.alloc(width * height * 4);
	let prev = Buffer.alloc(stride);
	for (let y = 0; y < height; y++) {
		const filter = raw[y * (stride + 1)];
		const line = Buffer.from(raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1)));
		for (let x = 0; x < stride; x++) {
			const a = x >= bpp ? line[x - bpp] : 0;
			const b = prev[x];
			const c = x >= bpp ? prev[x - bpp] : 0;
			let v = line[x];
			if (filter === 1) v += a;
			else if (filter === 2) v += b;
			else if (filter === 3) v += (a + b) >> 1;
			else if (filter === 4) {
				const p = a + b - c;
				const pa = Math.abs(p - a);
				const pb = Math.abs(p - b);
				const pc = Math.abs(p - c);
				v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
			}
			line[x] = v & 0xff;
		}
		for (let x = 0; x < width; x++) {
			const o = (y * width + x) * 4;
			if (bpp >= 3) {
				out[o] = line[x * bpp];
				out[o + 1] = line[x * bpp + 1];
				out[o + 2] = line[x * bpp + 2];
				out[o + 3] = bpp === 4 ? line[x * bpp + 3] : 255;
			} else {
				out[o] = out[o + 1] = out[o + 2] = line[x * bpp];
				out[o + 3] = bpp === 2 ? line[x * bpp + 1] : 255;
			}
		}
		prev = line;
	}
	return { width, height, data: out };
}

function crc32(buf) {
	let c = ~0;
	for (const byte of buf) {
		c ^= byte;
		for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
	}
	return ~c >>> 0;
}

function encode(width, height, rgba) {
	const chunk = (type, data) => {
		const len = Buffer.alloc(4);
		len.writeUInt32BE(data.length);
		const td = Buffer.concat([Buffer.from(type, 'ascii'), data]);
		const crc = Buffer.alloc(4);
		crc.writeUInt32BE(crc32(td));
		return Buffer.concat([len, td, crc]);
	};
	const ihdr = Buffer.alloc(13);
	ihdr.writeUInt32BE(width, 0);
	ihdr.writeUInt32BE(height, 4);
	ihdr[8] = 8;
	ihdr[9] = 6;
	const raw = Buffer.alloc(height * (width * 4 + 1));
	for (let y = 0; y < height; y++)
		rgba.copy(raw, y * (width * 4 + 1) + 1, y * width * 4, (y + 1) * width * 4);
	return Buffer.concat([
		Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
		chunk('IHDR', ihdr),
		chunk('IDAT', deflateSync(raw)),
		chunk('IEND', Buffer.alloc(0))
	]);
}

const rows = [];
for (const file of readdirSync(dir).filter((f) => f.endsWith('__legacy.png'))) {
	const base = file.slice(0, -'__legacy.png'.length);
	let rust;
	try {
		rust = decode(join(dir, `${base}__rust.png`));
	} catch {
		rows.push({ base, ratio: 1, note: 'rust fehlt' });
		continue;
	}
	const legacy = decode(join(dir, file));
	const width = Math.max(legacy.width, rust.width);
	const height = Math.max(legacy.height, rust.height);
	const out = Buffer.alloc(width * height * 4);
	let differing = 0;
	for (let y = 0; y < height; y++) {
		for (let x = 0; x < width; x++) {
			const o = (y * width + x) * 4;
			const inL = x < legacy.width && y < legacy.height;
			const inR = x < rust.width && y < rust.height;
			if (!inL || !inR) {
				out.set([255, 0, 255, 255], o);
				differing++;
				continue;
			}
			const lo = (y * legacy.width + x) * 4;
			const ro = (y * rust.width + x) * 4;
			const d =
				Math.abs(legacy.data[lo] - rust.data[ro]) +
				Math.abs(legacy.data[lo + 1] - rust.data[ro + 1]) +
				Math.abs(legacy.data[lo + 2] - rust.data[ro + 2]);
			if (d > 24) {
				out.set([255, 0, 0, 255], o);
				differing++;
			} else {
				const g = (legacy.data[lo] + legacy.data[lo + 1] + legacy.data[lo + 2]) / 3;
				const v = Math.round(255 - (255 - g) * 0.25);
				out.set([v, v, v, 255], o);
			}
		}
	}
	writeFileSync(join(dir, `${base}__diff.png`), encode(width, height, out));
	const note =
		legacy.width !== rust.width || legacy.height !== rust.height
			? `Größe legacy ${legacy.width}x${legacy.height}, rust ${rust.width}x${rust.height}`
			: '';
	rows.push({ base, ratio: differing / (width * height), note });
}
rows.sort((a, b) => b.ratio - a.ratio);
const report = rows
	.map((r) => `${(r.ratio * 100).toFixed(2).padStart(6)} %  ${r.base}  ${r.note}`)
	.join('\n');
writeFileSync(join(dir, 'report.txt'), report + '\n');
console.log(report);
