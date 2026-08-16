export interface Config {
	dataDir: string;
	coverDir: string;
	podcastsDir: string;
	databaseUrl: string;
}

export function loadConfig(env: Record<string, string | undefined>): Config {
	const dataDir = env.CAPSTAN_DATA ?? './data';
	return {
		dataDir,
		coverDir: `${dataDir}/covers`,
		podcastsDir: `${dataDir}/podcasts`,
		databaseUrl: env.DATABASE_URL ?? 'postgresql://capstan:capstan@localhost:5434/capstan'
	};
}
