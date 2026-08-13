export interface Config {
	dataDir: string;
	booksDir: string;
	musicDir: string;
	coverDir: string;
	podcastsDir: string;
	databaseUrl: string;
}

export function loadConfig(env: Record<string, string | undefined>): Config {
	const booksDir = env.CAPSTAN_BOOKS;
	if (!booksDir) throw new Error('CAPSTAN_BOOKS is not set');
	const musicDir = env.CAPSTAN_MUSIC;
	if (!musicDir) throw new Error('CAPSTAN_MUSIC is not set');
	const dataDir = env.CAPSTAN_DATA ?? './data';
	return {
		dataDir,
		booksDir,
		musicDir,
		coverDir: `${dataDir}/covers`,
		podcastsDir: `${dataDir}/podcasts`,
		databaseUrl: env.DATABASE_URL ?? 'postgresql://capstan:capstan@localhost:5433/capstan'
	};
}
