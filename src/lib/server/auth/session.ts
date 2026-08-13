import { error, redirect } from '@sveltejs/kit';
import { eq } from 'drizzle-orm';
import { users } from '../db/schema';
import { db } from '../db';

export const SESSION_COOKIE = 'capstan_session';

export interface Locals {
	userId: number | null;
	token: string | null;
}

/** Bearer header first (API clients), else the session cookie (web UI) — one resolver for both. */
export function tokenFromRequest(request: Request, cookieToken: string | null): string | null {
	const authorization = request.headers.get('authorization') ?? '';
	const [scheme, value] = authorization.split(' ');
	if (scheme?.toLowerCase() === 'bearer' && value) return value;
	return cookieToken;
}

export function requireApiUser(locals: Locals): number {
	if (locals.userId === null) throw error(401, 'Kein Token');
	return locals.userId;
}

export async function requireApiAdmin(locals: Locals): Promise<number> {
	const userId = requireApiUser(locals);
	const [row] = await db.select({ isAdmin: users.isAdmin }).from(users).where(eq(users.id, userId));
	if (!row?.isAdmin) throw error(403, 'Nur für Verwalter');
	return userId;
}

export function requireWebUser(locals: Locals): number {
	if (locals.userId === null) throw redirect(303, '/login');
	return locals.userId;
}
