import { eq } from 'drizzle-orm';
import { users } from '../db/schema';
import type { DrizzleDb } from '../db';

export const SPEED_MIN = 0.5;
export const SPEED_MAX = 4.0;
export const SKIP_MIN = 1;
export const SKIP_MAX = 300;
const DEFAULTS = { playbackSpeed: 1.0, skipBack: 30, skipForward: 15 };

export const clampSpeed = (v: number) => Math.min(SPEED_MAX, Math.max(SPEED_MIN, v));
export const clampSkip = (v: number) => Math.min(SKIP_MAX, Math.max(SKIP_MIN, Math.trunc(v)));

export async function getPreferences(db: DrizzleDb, userId: number) {
	const [row] = await db
		.select({
			playbackSpeed: users.playbackSpeed,
			skipBack: users.skipBack,
			skipForward: users.skipForward
		})
		.from(users)
		.where(eq(users.id, userId));
	return row ?? { ...DEFAULTS };
}

export async function setPreferences(
	db: DrizzleDb,
	userId: number,
	playbackSpeed: number,
	skipBack: number,
	skipForward: number
) {
	const [row] = await db
		.update(users)
		.set({
			playbackSpeed: clampSpeed(playbackSpeed),
			skipBack: clampSkip(skipBack),
			skipForward: clampSkip(skipForward)
		})
		.where(eq(users.id, userId))
		.returning({
			playbackSpeed: users.playbackSpeed,
			skipBack: users.skipBack,
			skipForward: users.skipForward
		});
	return row ?? { ...DEFAULTS };
}
