import { describe, it, expect } from 'vitest';
import { withTestDb } from '../fixtures';
import { createUser } from '../../src/lib/server/auth/passwords';
import { performLogin } from '../../src/lib/server/auth/login';
import { LoginThrottle } from '../../src/lib/server/auth/throttle';
import { Semaphore } from '../../src/lib/server/auth/semaphore';

describe('performLogin', () => {
	it('returns a token on correct credentials', async () => {
		await withTestDb(async (db) => {
			await createUser(db, 'oliver', 'hunter2hunter2');
			const throttle = new LoginThrottle();
			const semaphore = new Semaphore(4);
			const token = await performLogin(db, throttle, semaphore, 'oliver', 'hunter2hunter2');
			expect(token.length).toBeGreaterThan(30);
		});
	});

	it('throws 401 "Anmeldung fehlgeschlagen" on wrong password, still recording the attempt', async () => {
		await withTestDb(async (db) => {
			await createUser(db, 'oliver', 'hunter2hunter2');
			const throttle = new LoginThrottle();
			const semaphore = new Semaphore(4);
			await expect(performLogin(db, throttle, semaphore, 'oliver', 'wrong')).rejects.toMatchObject({
				status: 401,
				detail: 'Anmeldung fehlgeschlagen'
			});
		});
	});

	it('throws 429 after the throttle limit and releases the semaphore either way', async () => {
		await withTestDb(async (db) => {
			await createUser(db, 'oliver', 'hunter2hunter2');
			const throttle = new LoginThrottle();
			const semaphore = new Semaphore(4);
			for (let i = 0; i < 10; i++) {
				await performLogin(db, throttle, semaphore, 'oliver', 'wrong').catch(() => {});
			}
			await expect(performLogin(db, throttle, semaphore, 'oliver', 'wrong')).rejects.toMatchObject({
				status: 429
			});
			// semaphore was released on every prior call, so a fresh call can still acquire it
			expect(semaphore.tryAcquire()).toBe(true);
		});
	});

	it('throws 503 when the semaphore is exhausted, without touching the throttle', async () => {
		await withTestDb(async (db) => {
			await createUser(db, 'oliver', 'hunter2hunter2');
			const throttle = new LoginThrottle();
			const semaphore = new Semaphore(1);
			semaphore.tryAcquire(); // exhaust it
			await expect(
				performLogin(db, throttle, semaphore, 'oliver', 'hunter2hunter2')
			).rejects.toMatchObject({
				status: 503
			});
			expect(throttle.size()).toBe(0); // untouched
		});
	});
}, 60_000);
