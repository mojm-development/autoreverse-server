const LIMIT = 10;
const WINDOW_SECONDS = 300;
const MAX_KEYS = 10_000;

export class LoginThrottle {
	private failures = new Map<string, number[]>();
	private readonly limit: number;
	private readonly windowSeconds: number;
	private readonly maxKeys: number;
	private readonly now: () => number;

	constructor(opts?: {
		limit?: number;
		windowSeconds?: number;
		maxKeys?: number;
		now?: () => number;
	}) {
		this.limit = opts?.limit ?? LIMIT;
		this.windowSeconds = opts?.windowSeconds ?? WINDOW_SECONDS;
		this.maxKeys = opts?.maxKeys ?? MAX_KEYS;
		this.now = opts?.now ?? (() => Date.now() / 1000);
	}

	private prune(key: string): number[] {
		const cutoff = this.now() - this.windowSeconds;
		const existing = this.failures.get(key) ?? [];
		const kept = existing.filter((t) => t > cutoff);
		if (kept.length === 0) this.failures.delete(key);
		else this.failures.set(key, kept);
		return kept;
	}

	private evictExpired(): void {
		for (const [key, timestamps] of this.failures) {
			const cutoff = this.now() - this.windowSeconds;
			if (!timestamps.some((t) => t > cutoff)) this.failures.delete(key);
		}
	}

	private appendFailure(key: string, timestamp: number): void {
		const isNewKey = !this.failures.has(key);
		if (isNewKey && this.failures.size >= this.maxKeys) {
			this.evictExpired();
			if (this.failures.size >= this.maxKeys) return;
		}
		const timestamps = this.failures.get(key) ?? [];
		timestamps.push(timestamp);
		this.failures.delete(key);
		this.failures.set(key, timestamps);
	}

	checkAndRecordAttempt(key: string): number | null {
		const timestamps = this.prune(key);
		if (timestamps.length >= this.limit) {
			const oldest = timestamps[0];
			return Math.max(0, oldest + this.windowSeconds - this.now());
		}
		this.appendFailure(key, this.now());
		return null;
	}

	recordSuccess(key: string): void {
		this.failures.delete(key);
	}

	recordFailure(key: string): void {
		this.prune(key);
		this.appendFailure(key, this.now());
	}

	check(key: string): number | null {
		const timestamps = this.prune(key);
		if (timestamps.length >= this.limit) {
			return Math.max(0, timestamps[0] + this.windowSeconds - this.now());
		}
		return null;
	}

	size(): number {
		return this.failures.size;
	}
}
