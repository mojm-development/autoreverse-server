import { hash, verify } from '@node-rs/argon2';
import { eq, sql } from 'drizzle-orm';
import { users } from '../db/schema';
import type { DrizzleDb } from '../db';

const ARGON2_OPTIONS = {
	algorithm: 2,
	memoryCost: 65536,
	timeCost: 3,
	parallelism: 4,
	outputLen: 32
};

export function hashPassword(plain: string): Promise<string> {
	return hash(plain, ARGON2_OPTIONS);
}

export async function verifyPassword(hashed: string, plain: string): Promise<boolean> {
	try {
		return await verify(hashed, plain);
	} catch {
		return false;
	}
}

export async function createUser(
	db: DrizzleDb,
	name: string,
	password: string,
	isAdmin = false
): Promise<number> {
	return db.transaction(async (tx) => {
		const [{ count }] = await tx.select({ count: sql<number>`count(*)::int` }).from(users);
		const isFirst = count === 0;
		const [row] = await tx
			.insert(users)
			.values({ name, passwordHash: await hashPassword(password), isAdmin: isAdmin || isFirst })
			.returning({ id: users.id });
		return row.id;
	});
}

export async function setPassword(db: DrizzleDb, userId: number, password: string): Promise<void> {
	await db
		.update(users)
		.set({ passwordHash: await hashPassword(password) })
		.where(eq(users.id, userId));
}

export async function authenticate(
	db: DrizzleDb,
	name: string,
	password: string
): Promise<number | null> {
	const [row] = await db
		.select({ id: users.id, passwordHash: users.passwordHash })
		.from(users)
		.where(eq(users.name, name));
	if (!row) {
		await hashPassword(password);
		return null;
	}
	return (await verifyPassword(row.passwordHash, password)) ? row.id : null;
}
