import { redirect } from '@sveltejs/kit';
import { eq } from 'drizzle-orm';
import { users } from '../db/schema';
import type { DrizzleDb } from '../db';
import { ApiError } from '../api/errors';

export const SESSION_COOKIE = 'autoreverse_session';

export interface Locals {
	userId: number | null;
	token: string | null;
}

export function tokenFromRequest(request: Request, cookieToken: string | null): string | null {
	const authorization = request.headers.get('authorization') ?? '';
	const [scheme, value] = authorization.split(' ');
	if (scheme?.toLowerCase() === 'bearer' && value) return value;
	return cookieToken;
}

export function requireApiUser(locals: Locals): number {
	if (locals.userId === null) throw new ApiError(401, 'Kein Token');
	return locals.userId;
}

export async function requireApiAdmin(locals: Locals, db: DrizzleDb): Promise<number> {
	const userId = requireApiUser(locals);
	const [row] = await db.select({ isAdmin: users.isAdmin }).from(users).where(eq(users.id, userId));
	if (!row?.isAdmin) throw new ApiError(403, 'Nur für Verwalter');
	return userId;
}

export async function requireWebAdmin(locals: Locals, db: DrizzleDb): Promise<number> {
	const userId = requireWebUser(locals);
	const [row] = await db.select({ isAdmin: users.isAdmin }).from(users).where(eq(users.id, userId));
	if (!row?.isAdmin) throw redirect(303, '/library');
	return userId;
}

export function requireWebUser(locals: Locals): number {
	if (locals.userId === null) throw redirect(303, '/login');
	return locals.userId;
}
