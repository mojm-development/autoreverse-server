import { and, eq, isNotNull, sql } from 'drizzle-orm';
import { db } from '$lib/server/db';
import { requireWebUser } from '$lib/server/auth/session';
import { items as itemsTable } from '$lib/server/db/schema';

export const load = async ({ locals }) => {
	requireWebUser(locals);
	const series = await db
		.select({ series: itemsTable.series, count: sql<number>`count(*)::int` })
		.from(itemsTable)
		.where(and(eq(itemsTable.kind, 'book'), isNotNull(itemsTable.series)))
		.groupBy(itemsTable.series)
		.orderBy(sql`lower(${itemsTable.series})`);
	return { series };
};
