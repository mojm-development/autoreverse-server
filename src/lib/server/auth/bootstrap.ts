import { sql } from 'drizzle-orm';
import { users } from '../db/schema';
import { createUser } from './passwords';
import type { DrizzleDb } from '../db';

const ADMIN_USER_VAR = 'CAPSTAN_ADMIN_USER';
const ADMIN_PASSWORD_VAR = 'CAPSTAN_ADMIN_PASSWORD';

/** Creates the first admin from env vars, only if the users table is still empty. */
export async function ensureFirstAdmin(
	db: DrizzleDb,
	env: Record<string, string | undefined>
): Promise<void> {
	const [{ count }] = await db.select({ count: sql<number>`count(*)::int` }).from(users);
	if (count > 0) return;
	const name = env[ADMIN_USER_VAR];
	const password = env[ADMIN_PASSWORD_VAR];
	if (!name || !password) {
		console.error(
			`Kein Nutzer vorhanden und ${ADMIN_USER_VAR}/${ADMIN_PASSWORD_VAR} nicht gesetzt — ` +
				'Server startet trotzdem, aber niemand kann sich anmelden.'
		);
		return;
	}
	await createUser(db, name, password, true);
	console.error(`Ersten Verwalter '${name}' aus der Umgebung angelegt.`);
}
