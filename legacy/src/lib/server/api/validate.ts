import { ApiError } from './errors';

export async function readJson<T = unknown>(request: Request): Promise<T> {
	try {
		return (await request.json()) as T;
	} catch {
		throw new ApiError(422, 'Ungültiger JSON-Body');
	}
}

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
