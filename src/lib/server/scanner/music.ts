import { scanTree } from './books';
import type { ScannedItem, ScanFailure } from './books';

export async function scanMusic(
	root: string,
	known: Record<string, [number, number]>,
	failures: ScanFailure[] = []
): Promise<ScannedItem[]> {
	return scanTree(root, 'album', known, failures);
}
