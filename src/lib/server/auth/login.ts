import { error } from '@sveltejs/kit';
import { authenticate } from './passwords';
import { issueToken } from './tokens';
import type { LoginThrottle } from './throttle';
import type { Semaphore } from './semaphore';
import type { DrizzleDb } from '../db';

/**
 * Exact port of capstan/src/capstan/api/auth.py::perform_login. Ordering is
 * load-bearing: semaphore acquired non-blocking FIRST (before touching the
 * throttle at all — acquiring throttle-first would record a failed attempt
 * even for requests only rejected due to concurrency pressure), throttle
 * checked+recorded INSIDE the semaphore-held section and BEFORE the argon2
 * call (closes the check-then-record race), semaphore released in `finally`
 * around the whole throttle+authenticate block on every exit path.
 */
export async function performLogin(
	db: DrizzleDb,
	throttle: LoginThrottle,
	semaphore: Semaphore,
	name: string,
	password: string
): Promise<string> {
	if (!semaphore.tryAcquire()) {
		throw error(503, {
			message: 'Gerade zu viele gleichzeitige Anmeldeversuche, bitte kurz erneut versuchen'
		});
	}
	let userId: number | null;
	try {
		const key = name.toLowerCase();
		const remaining = throttle.checkAndRecordAttempt(key);
		if (remaining !== null) {
			throw error(429, { message: 'Zu viele Fehlversuche, bitte später erneut versuchen' });
		}
		userId = await authenticate(db, name, password);
	} finally {
		semaphore.release();
	}
	if (userId === null) throw error(401, { message: 'Anmeldung fehlgeschlagen' });
	throttle.recordSuccess(name.toLowerCase());
	return issueToken(db, userId);
}
