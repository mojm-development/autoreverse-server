import { allDirectories, scanFolder } from './books';
import type { ScannedItem } from './books';

export async function scanMusic(
	root: string,
	known: Record<string, [number, number]>
): Promise<ScannedItem[]> {
	const dirs = await allDirectories(root);
	const results: ScannedItem[] = [];
	for (const dir of dirs) {
		const scanned = await scanFolder(root, dir, 'album', known);
		if (scanned) results.push(scanned);
	}
	return results;
}
