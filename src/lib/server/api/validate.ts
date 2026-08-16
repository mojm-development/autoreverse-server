import { ApiError } from './errors';

/**
 * Parses a JSON request body, throwing a Ruling-8 `ApiError(422, ...)` on
 * malformed/empty bodies instead of letting `SyntaxError` bubble up as an
 * unhandled 500. Use in place of a bare `await request.json()` in every
 * route handler.
 */
export async function readJson<T = unknown>(request: Request): Promise<T> {
	try {
		return (await request.json()) as T;
	} catch {
		throw new ApiError(422, 'Ungültiger JSON-Body');
	}
}

/**
 * Reads an integer query param, applying the ground truth's `Query(ge=…, le=…)`
 * semantics: missing -> default, out-of-range/non-numeric -> 422 (not a silent clamp).
 */
export function intParam(
	url: URL,
	name: string,
	opts: { def: number; min: number; max: number }
): number {
	const raw = url.searchParams.get(name);
	if (raw === null) return opts.def;
	const n = Number(raw);
	if (!Number.isFinite(n) || !Number.isInteger(n) || n < opts.min || n > opts.max) {
		throw new ApiError(422, `${name} außerhalb des gültigen Bereichs`);
	}
	return n;
}
