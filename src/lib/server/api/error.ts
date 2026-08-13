/**
 * Build a consistent API error Response with { detail: string } body.
 * Used by all API routes to ensure uniform error response format.
 */
export function apiError(status: number, detail: string, retryAfter?: number | string): Response {
	const headers: Record<string, string> = {
		'content-type': 'application/json'
	};
	if (retryAfter !== undefined) {
		headers['retry-after'] = String(retryAfter);
	}
	return new Response(JSON.stringify({ detail }), { status, headers });
}
