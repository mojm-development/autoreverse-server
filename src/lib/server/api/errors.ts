/**
 * Custom error class for API errors that carry metadata like Retry-After values.
 */
export class ApiError extends Error {
	constructor(
		public readonly status: number,
		public readonly detail: string,
		public readonly retryAfter?: number | string
	) {
		super(detail);
		this.name = 'ApiError';
	}
}
