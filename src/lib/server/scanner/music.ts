import { scanTree } from './books';
import type { ScannedItem, ScanFailure } from './books';
import type { ProgressFn } from '../admin/scanState';

export async function scanMusic(
	root: string,
	known: Record<string, [number, number]>,
	failures: ScanFailure[] = [],
	onProgress?: ProgressFn
): Promise<ScannedItem[]> {
	return scanTree(root, 'album', known, failures, onProgress);
}
