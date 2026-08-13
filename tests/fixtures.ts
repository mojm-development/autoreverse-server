import { PostgreSqlContainer, type StartedPostgreSqlContainer } from '@testcontainers/postgresql';
import { drizzle } from 'drizzle-orm/postgres-js';
import postgres from 'postgres';
import { execSync } from 'node:child_process';
import * as schema from '../src/lib/server/db/schema';

export async function withTestDb<T>(
	fn: (db: ReturnType<typeof drizzle<typeof schema>>) => Promise<T>
): Promise<T> {
	const container: StartedPostgreSqlContainer = await new PostgreSqlContainer(
		'postgres:16'
	).start();
	const url = container.getConnectionUri();
	try {
		execSync('node node_modules/drizzle-kit/bin.cjs push --force', {
			env: { ...process.env, DATABASE_URL: url },
			stdio: 'inherit'
		});
		const client = postgres(url);
		const db = drizzle(client, { schema });
		try {
			return await fn(db);
		} finally {
			await client.end();
		}
	} finally {
		await container.stop();
	}
}
