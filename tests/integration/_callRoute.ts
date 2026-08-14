import type { DrizzleDb } from '../../src/lib/server/db';
import type { Locals } from '../../src/lib/server/auth/session';

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export async function callRoute(
	handler: (db: DrizzleDb, event: any) => Promise<Response>,
	opts: {
		db: DrizzleDb;
		locals: Locals;
		params?: Record<string, string>;
		body?: unknown;
		url?: string;
	}
) {
	const request =
		opts.body !== undefined
			? new Request(opts.url ?? 'http://test/', {
					method: 'POST',
					body: JSON.stringify(opts.body),
					headers: { 'content-type': 'application/json' }
				})
			: new Request(opts.url ?? 'http://test/');
	const event = {
		request,
		locals: opts.locals,
		params: opts.params ?? {},
		url: new URL(opts.url ?? 'http://test/')
	};
	return handler(opts.db, event);
}
