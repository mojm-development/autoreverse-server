import { LoginThrottle } from './throttle';

export class Semaphore {
	private available: number;
	constructor(private readonly max: number) {
		this.available = max;
	}
	tryAcquire(): boolean {
		if (this.available <= 0) return false;
		this.available -= 1;
		return true;
	}
	release(): void {
		this.available = Math.min(this.max, this.available + 1);
	}
}

export const MAX_CONCURRENT_LOGIN_HASHES = 4;
export const loginHashSemaphore = new Semaphore(MAX_CONCURRENT_LOGIN_HASHES);
export const loginThrottle = new LoginThrottle();
