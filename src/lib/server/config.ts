export interface Config {
	dataDir: string;
	coverDir: string;
	podcastsDir: string;
	artistsDir: string;
	podcastRefreshHours: number;
	databaseUrl: string;
}

export function loadConfig(env: Record<string, string | undefined>): Config {
	const dataDir = env.AUTOREVERSE_DATA ?? './data';
	return {
		dataDir,
		coverDir: `${dataDir}/covers`,
		podcastsDir: `${dataDir}/podcasts`,
		artistsDir: `${dataDir}/artists`,
		podcastRefreshHours: Math.max(0, Number(env.AUTOREVERSE_PODCAST_REFRESH_HOURS ?? 6) || 0),
		databaseUrl:
			env.DATABASE_URL ?? 'postgresql://autoreverse:autoreverse@localhost:5434/autoreverse'
	};
}
