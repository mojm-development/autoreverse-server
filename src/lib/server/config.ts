export interface Config {
	dataDir: string;
	coverDir: string;
	podcastsDir: string;
	artistsDir: string;
	databaseUrl: string;
}

export function loadConfig(env: Record<string, string | undefined>): Config {
	const dataDir = env.AUTOREVERSE_DATA ?? './data';
	return {
		dataDir,
		coverDir: `${dataDir}/covers`,
		podcastsDir: `${dataDir}/podcasts`,
		artistsDir: `${dataDir}/artists`,
		databaseUrl:
			env.DATABASE_URL ?? 'postgresql://autoreverse:autoreverse@localhost:5434/autoreverse'
	};
}
