import { expect, type APIRequestContext } from '@playwright/test';

// Shared helpers, not a spec (testMatch is *.e2e.ts). The fixture items themselves are created by
// e2e/seed.sql on the server side; the browser VM has no database access.

export const TEST_USER = 'Oliver';
export const TEST_PASSWORD = 'hunter2hunter2';
export const NONADMIN_TEST_USER = 'NonAdmin';
export const NONADMIN_TEST_PASSWORD = 'hunter2hunter2';

/** Id of the seeded item with exactly this title, looked up over the authenticated API. */
export async function itemId(request: APIRequestContext, title: string): Promise<number> {
	const res = await request.get(`/items?q=${encodeURIComponent(title)}`);
	expect(res.ok(), `GET /items?q=${title}`).toBe(true);
	const { items } = (await res.json()) as { items: { id: number; title: string }[] };
	const item = items.find((i) => i.title === title);
	if (!item) throw new Error(`Fixture "${title}" fehlt – wurde e2e/seed.sql angewandt?`);
	return item.id;
}
