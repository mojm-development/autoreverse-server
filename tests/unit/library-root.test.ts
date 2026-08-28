import { describe, it, expect } from 'vitest';
import { mkdtempSync, writeFileSync, mkdirSync, chmodSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { libraryRootProblem } from '../../src/lib/server/scanner/books';

/** The scanner deliberately swallows readdir failures per folder. On the root
 * itself that silence is indistinguishable from an empty library, which is why
 * runScan asks first — an unreadable root must not reach markMissing. */
describe('libraryRootProblem', () => {
	it('passes a readable directory', async () => {
		const dir = mkdtempSync(join(tmpdir(), 'autoreverse-root-'));
		expect(await libraryRootProblem(dir)).toBeNull();
	});

	it('passes an empty directory — empty is not a problem', async () => {
		const dir = mkdtempSync(join(tmpdir(), 'autoreverse-root-'));
		mkdirSync(join(dir, 'leer'));
		expect(await libraryRootProblem(join(dir, 'leer'))).toBeNull();
	});

	it('reports a path that does not exist', async () => {
		const dir = mkdtempSync(join(tmpdir(), 'autoreverse-root-'));
		expect(await libraryRootProblem(join(dir, 'weg'))).toBe('Verzeichnis existiert nicht');
	});

	it('reports a path that is a file', async () => {
		const dir = mkdtempSync(join(tmpdir(), 'autoreverse-root-'));
		const file = join(dir, 'datei.txt');
		writeFileSync(file, '');
		expect(await libraryRootProblem(file)).toBe('Pfad ist kein Verzeichnis');
	});

	it.skipIf(process.getuid?.() === 0)('reports a directory it may not read', async () => {
		const dir = mkdtempSync(join(tmpdir(), 'autoreverse-root-'));
		const locked = join(dir, 'gesperrt');
		mkdirSync(locked);
		chmodSync(locked, 0o000);
		try {
			expect(await libraryRootProblem(locked)).toBe('Verzeichnis ist nicht lesbar');
		} finally {
			chmodSync(locked, 0o700);
		}
	});
});
