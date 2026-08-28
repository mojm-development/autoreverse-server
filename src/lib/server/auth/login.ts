import { ApiError } from '../api/errors';
import { authenticate } from './passwords';
import { issueToken } from './tokens';
import type { LoginThrottle } from './throttle';
import type { Semaphore } from './semaphore';
import type { DrizzleDb } from '../db';

export async function performLogin(
	db: DrizzleDb,
	throttle: LoginThrottle,
	semaphore: Semaphore,
	name: string,
	password: string
): Promise<string> {
	if (!semaphore.tryAcquire()) {
		throw new ApiError(
			503,
			'Gerade zu viele gleichzeitige Anmeldeversuche, bitte kurz erneut versuchen',
			1
		);
	}
	let userId: number | null;
	try {
		const key = name.toLowerCase();
		const remaining = throttle.checkAndRecordAttempt(key);
		if (remaining !== null) {
			throw new ApiError(
				429,
				'Zu viele Fehlversuche, bitte später erneut versuchen',
				Math.ceil(remaining)
			);
		}
		userId = await authenticate(db, name, password);
	} finally {
		semaphore.release();
	}
	if (userId === null) throw new ApiError(401, 'Anmeldung fehlgeschlagen');
	throttle.recordSuccess(name.toLowerCase());
	return issueToken(db, userId);
}
