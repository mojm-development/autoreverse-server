import type { DrizzleDb } from '../../src/lib/server/db';
import type { Locals } from '../../src/lib/server/auth/session';

interface FakeRequestEvent {
	request: Request;
	locals: Locals;
	params: Record<string, string>;
	url: URL;
}

export async function callRoute(
	handler: (db: DrizzleDb, event: FakeRequestEvent) => Promise<Response>,
	opts: {
		db: DrizzleDb;
		locals: Locals;
		params?: Record<string, string>;
		body?: unknown;
		url?: string;
		headers?: Record<string, string>;
	}
) {
	const headers = new Headers(opts.headers ?? {});
	if (opts.body !== undefined) {
		headers.set('content-type', 'application/json');
	}
	const request =
		opts.body !== undefined
			? new Request(opts.url ?? 'http://test/', {
					method: 'POST',
					body: JSON.stringify(opts.body),
					headers
				})
			: new Request(opts.url ?? 'http://test/', { headers });
	const event: FakeRequestEvent = {
		request,
		locals: opts.locals,
		params: opts.params ?? {},
		url: new URL(opts.url ?? 'http://test/')
	};
	return handler(opts.db, event);
}
