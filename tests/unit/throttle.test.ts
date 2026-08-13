import { describe, it, expect } from 'vitest';
import { LoginThrottle } from '../../src/lib/server/auth/throttle';

describe('LoginThrottle', () => {
	it('allows attempts under the limit', () => {
		const now = 0;
		const throttle = new LoginThrottle({ now: () => now });
		for (let i = 0; i < 10; i++) {
			expect(throttle.checkAndRecordAttempt('oliver')).toBeNull();
		}
	});

	it('locks out after the 11th attempt within the window', () => {
		const now = 0;
		const throttle = new LoginThrottle({ now: () => now });
		for (let i = 0; i < 10; i++) throttle.checkAndRecordAttempt('oliver');
		const remaining = throttle.checkAndRecordAttempt('oliver');
		expect(remaining).toBeGreaterThan(0);
		expect(remaining).toBeLessThanOrEqual(300);
	});

	it('slides the window: attempts older than 300s no longer count', () => {
		let now = 0;
		const throttle = new LoginThrottle({ now: () => now });
		for (let i = 0; i < 10; i++) throttle.checkAndRecordAttempt('oliver');
		now = 301;
		expect(throttle.checkAndRecordAttempt('oliver')).toBeNull();
	});

	it('recordSuccess clears history for the key', () => {
		const now = 0;
		const throttle = new LoginThrottle({ now: () => now });
		for (let i = 0; i < 10; i++) throttle.checkAndRecordAttempt('oliver');
		throttle.recordSuccess('oliver');
		expect(throttle.checkAndRecordAttempt('oliver')).toBeNull();
	});

	it('keys are independent', () => {
		const now = 0;
		const throttle = new LoginThrottle({ now: () => now });
		for (let i = 0; i < 10; i++) throttle.checkAndRecordAttempt('oliver');
		expect(throttle.checkAndRecordAttempt('mara')).toBeNull();
	});
});
