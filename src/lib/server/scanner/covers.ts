import { join } from 'node:path';
import { readdir, writeFile, rename, mkdir } from 'node:fs/promises';

const NAMES = ['cover', 'folder', 'front', 'album', 'albumart'];
const SUFFIXES = ['.jpg', '.jpeg', '.png', '.webp'];

export async function findCoverFile(dir: string): Promise<string | null> {
	let entries: { name: string; isFile: () => boolean }[];
	try {
		entries = await readdir(dir, { withFileTypes: true });
	} catch {
		return null;
	}
	const lowerToActual = new Map(
		entries.filter((e) => e.isFile()).map((e) => [e.name.toLowerCase(), e.name])
	);
	for (const name of NAMES) {
		for (const suffix of SUFFIXES) {
			const actual = lowerToActual.get(`${name}${suffix}`);
			if (actual) return join(dir, actual);
		}
	}
	return null;
}

const MAGIC: Array<[Buffer, string]> = [
	[Buffer.from([0xff, 0xd8, 0xff]), '.jpg'],
	[Buffer.from('89504e470d0a1a0a', 'hex'), '.png'],
	[Buffer.from('RIFF'), '.webp'],
	[Buffer.from('GIF8'), '.gif']
];

function sniffSuffix(data: Buffer): string {
	for (const [magic, suffix] of MAGIC) {
		if (data.subarray(0, magic.length).equals(magic)) return suffix;
	}
	return '.bin';
}

export async function extractEmbedded(
	trackPath: string,
	coversDir: string,
	itemId: number
): Promise<string | null> {
	const { parseFile } = await import('music-metadata');
	let picture: Buffer | null = null;
	try {
		const meta = await parseFile(trackPath);
		const pic = meta.common.picture?.[0];
		if (pic) picture = Buffer.from(pic.data);
	} catch {
		return null;
	}
	if (!picture) return null;

	return writeCoverBytes(coversDir, itemId, picture);
}

export async function writeCoverBytes(
	coversDir: string,
	itemId: number,
	data: Buffer
): Promise<string> {
	await mkdir(coversDir, { recursive: true });
	const destination = join(coversDir, `${itemId}${sniffSuffix(data)}`);
	const temp = `${destination}.part`;
	await writeFile(temp, data);
	await rename(temp, destination);
	return destination;
}
