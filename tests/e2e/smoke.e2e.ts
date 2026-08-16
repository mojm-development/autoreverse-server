import { expect, test } from '@playwright/test';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { eq } from 'drizzle-orm';
import { db } from '../../src/lib/server/db';
import {
	items as itemsTable,
	libraryConfig as libraryConfigTable
} from '../../src/lib/server/db/schema';

const LIBRARY_CONFIG_ROW_ID = 1;

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const BOOKS_DIR = path.resolve(__dirname, '../fixtures/library/books');
const MUSIC_DIR = path.resolve(__dirname, '../fixtures/library/music');

// This is the scanner's own `sourcePath` for each fixture folder:
// scanFolder() sets `sourcePath: dir`, where `dir` comes from allDirectories(root)
// walking via `join(root, ...)` — see src/lib/server/scanner/books.ts (scanMusic
// reuses the same scanFolder/allDirectories, just with kind: 'album').
const FIXTURE_BOOK_SOURCE_PATH = path.join(BOOKS_DIR, 'Autor', 'Buch');
const FIXTURE_ALBUM_SOURCE_PATH = path.join(MUSIC_DIR, 'Interpret', 'Album');

// Runs without a pre-authenticated session — the whole point of this test is
// proving the login cookie genuinely round-trips through a real browser. Left
// to the default `authenticated` project it would inherit
// storageState: 'playwright/.auth/user.json' and start already logged in,
// silently skipping the thing it's meant to test.
test.use({ storageState: { cookies: [], origins: [] } });

test('full journey: login, scan, browse, play, progress persists across reload', async ({
	page
}) => {
	// The default 30s test timeout is too tight alongside a 30s expect timeout
	// further down (waiting on the scan to actually finish against the real
	// filesystem/DB) plus everything before and after it in this one long
	// journey — extend it rather than touching playwright.config.ts's global
	// default, which is shared by every other e2e spec.
	test.setTimeout(60_000);

	// storeItems() upserts by sourcePath: a second scan of the same fixture
	// folder would report `updated`, not `new`. Delete any leftover items from
	// a prior run of this same test so the "new" count holds on every run, not
	// just the database's first-ever scan of these fixture paths
	// (items.sourcePath has a cascade-delete FK from tracks/chapters/progress,
	// so this is a clean single-statement reset per item).
	//
	// Both the book folder AND the music album folder are deleted (not just
	// the book): runScan() (src/lib/server/scanner/run.ts) accumulates a
	// SINGLE lastReport across both the books-dir and music-dir passes, so
	// leaving the album item alone would make it "unchanged" after the first
	// run while the book is freshly "new" — producing "neu 1" on some runs and
	// "neu 2" on others depending on run history. Resetting both fixture items
	// every time makes both always-new, so the combined report is
	// deterministically "neu 2" on every run.
	await db.delete(itemsTable).where(eq(itemsTable.sourcePath, FIXTURE_BOOK_SOURCE_PATH));
	await db.delete(itemsTable).where(eq(itemsTable.sourcePath, FIXTURE_ALBUM_SOURCE_PATH));

	// library_config is a single global singleton row (id=1, see
	// src/lib/server/settings/libraryPaths.ts) shared by the whole dev DB that
	// every other e2e spec also runs against — including
	// tests/e2e/settings.e2e.ts's "the scan status card ... reflects real
	// backend state" test, which specifically asserts the *unconfigured*-paths
	// error message. Overwriting it here without putting it back would
	// permanently break that test for every run after this one (not just a
	// same-run race). Capture whatever was there before so it can be restored.
	const [originalLibraryConfig] = await db
		.select()
		.from(libraryConfigTable)
		.where(eq(libraryConfigTable.id, LIBRARY_CONFIG_ROW_ID));

	await page.goto('/login');
	await page.getByLabel('Name', { exact: false }).fill('Oliver');
	await page.getByLabel('Passwort').fill('hunter2hunter2');
	await page.getByRole('button', { name: 'Anmelden', exact: true }).click();
	await expect(page).toHaveURL('/library');

	try {
		// Configure library paths through the real admin UI (Bibliotheken panel),
		// not a backdoor DB write — exercises Task 39's own save flow for real.
		await page.goto('/settings/libraries');
		await page.getByLabel('Bücher-Pfad', { exact: false }).fill(BOOKS_DIR);
		await page.getByLabel('Musik-Pfad', { exact: false }).fill(MUSIC_DIR);
		await page.getByRole('button', { name: 'Speichern' }).click();

		await page.goto('/settings/scan');
		await page.getByRole('button', { name: 'Scan starten' }).click();
		await expect(page.getByText('Scan läuft')).toBeVisible();
		await expect(page.getByText(/neu 2/)).toBeVisible({ timeout: 30_000 });
	} finally {
		// Restore immediately after the scan completes (rather than only at the
		// very end of the test) to keep the window where other, parallel-worker
		// e2e specs could observe our fixture paths as small as possible — the
		// remaining browse/play/reload steps below don't need library paths
		// configured at all, since the scanned items are already in the DB.
		if (originalLibraryConfig) {
			await db
				.update(libraryConfigTable)
				.set({
					booksDir: originalLibraryConfig.booksDir,
					musicDir: originalLibraryConfig.musicDir,
					updatedAt: originalLibraryConfig.updatedAt
				})
				.where(eq(libraryConfigTable.id, LIBRARY_CONFIG_ROW_ID));
		} else {
			await db.delete(libraryConfigTable).where(eq(libraryConfigTable.id, LIBRARY_CONFIG_ROW_ID));
		}
	}

	await page.goto('/library/books');
	await page.getByText('Buch').click();
	await expect(page).toHaveURL(/\/library\/books\/\d+/);
	await page.getByRole('button', { name: 'Abspielen' }).click();
	await expect(page.locator('.bar')).toBeVisible();

	await page.waitForTimeout(3_000); // let a little real position accumulate
	await page.reload();
	await expect(page.getByText(/Weiter ab/)).toBeVisible();
});
