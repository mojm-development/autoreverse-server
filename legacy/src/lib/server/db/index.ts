import { drizzle } from 'drizzle-orm/postgres-js';
import postgres from 'postgres';
import { loadConfig } from '../config';
import * as schema from './schema';

const config = loadConfig(process.env as Record<string, string | undefined>);
const client = postgres(config.databaseUrl);
export const db = drizzle(client, { schema });
export type DrizzleDb = typeof db;
