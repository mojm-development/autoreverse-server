/**
 * Setzt das Passwort eines Nutzers, oder legt ihn an.
 *
 * Gebraucht, weil `ensureFirstAdmin` nur greift, solange die Nutzertabelle leer
 * ist: eine Dev-Datenbank, die schon einen Admin hat, ignoriert jedes spätere
 * AUTOREVERSE_ADMIN_PASSWORD in der .env — und ohne dieses Skript käme man an
 * eine Datenbank mit vergessenem Passwort nur noch per SQL heran.
 *
 *   bun run scripts/set-password.ts <name> <passwort> [--admin]
 */
import { eq } from 'drizzle-orm';
import { db } from '../src/lib/server/db';
import { users } from '../src/lib/server/db/schema';
import { createUser, setPassword } from '../src/lib/server/auth/passwords';

const [name, password, ...rest] = process.argv.slice(2);
const admin = rest.includes('--admin');

if (!name || !password) {
	console.error('Aufruf: bun run scripts/set-password.ts <name> <passwort> [--admin]');
	process.exit(2);
}
if (password.length < 8) {
	console.error('Das Passwort muss mindestens 8 Zeichen haben.');
	process.exit(2);
}

const [existing] = await db.select().from(users).where(eq(users.name, name));
if (existing) {
	await setPassword(db, existing.id, password);
	if (admin && !existing.isAdmin) {
		await db.update(users).set({ isAdmin: true }).where(eq(users.id, existing.id));
	}
	console.log(`Passwort für "${name}" gesetzt${admin ? ' (Verwalter)' : ''}.`);
} else {
	await createUser(db, name, password, admin);
	console.log(`Nutzer "${name}" angelegt${admin ? ' (Verwalter)' : ''}.`);
}
process.exit(0);
