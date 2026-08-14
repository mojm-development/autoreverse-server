import type { RequestHandler } from '@sveltejs/kit';
import type { DrizzleDb } from '../../src/lib/server/db';
import type { Locals } from '../../src/lib/server/auth/session';

export async function callRoute(
	handler: RequestHandler,
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
		url: new URL(opts.url ?? 'http://test/'),
		cookies: { get: () => undefined, set: () => {} },
		platform: { context: { db: opts.db } }
	} as unknown;
	return handler(event as Parameters<RequestHandler>[0]);
}
