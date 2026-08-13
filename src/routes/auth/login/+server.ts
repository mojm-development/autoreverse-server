import { json, type RequestHandler } from '@sveltejs/kit';
import { db } from '$lib/server/db';
import { performLogin } from '$lib/server/auth/login';
import { loginThrottle, loginHashSemaphore } from '$lib/server/auth/semaphore';

export const POST: RequestHandler = async ({ request }) => {
	const { name, password } = await request.json();
	try {
		const token = await performLogin(db, loginThrottle, loginHashSemaphore, name, password);
		return json({ token });
	} catch (err) {
		const e = err as unknown as { status?: number; body?: { message?: string } };
		if (e?.status === 503) {
			return new Response(JSON.stringify({ detail: e.body?.message }), {
				status: 503,
				headers: { 'content-type': 'application/json', 'retry-after': '1' }
			});
		}
		if (e?.status === 429) {
			const remaining = Math.ceil(loginThrottle.check(name.toLowerCase()) ?? 0);
			return new Response(JSON.stringify({ detail: e.body?.message }), {
				status: 429,
				headers: { 'content-type': 'application/json', 'retry-after': String(remaining) }
			});
		}
		if (e?.status === 401) {
			return new Response(JSON.stringify({ detail: e.body?.message }), {
				status: 401,
				headers: { 'content-type': 'application/json' }
			});
		}
		throw err;
	}
};
