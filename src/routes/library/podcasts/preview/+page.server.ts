import { error, fail, redirect } from '@sveltejs/kit';
import { and, eq } from 'drizzle-orm';
import { db } from '$lib/server/db';
import { requireWebAdmin } from '$lib/server/auth/session';
import { loadConfig } from '$lib/server/config';
import { items as itemsTable } from '$lib/server/db/schema';
import { podcastOverview } from '$lib/server/library/queries';
import {
	previewFeed,
	subscribe,
	FeedFetchError,
	InvalidFeedError
} from '$lib/server/podcasts/store';

/** Only http(s) — the feed URL arrives from a query string, and previewFeed()
 *  passes it straight to fetch(), so file:/data: URLs must not get that far. */
function validFeedUrl(raw: string | null): string {
	if (!raw) throw error(400, 'Keine Feed-Adresse angegeben');
	let parsed: URL;
	try {
		parsed = new URL(raw);
	} catch {
		throw error(400, 'Ungültige Feed-Adresse');
	}
	if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:')
		throw error(400, 'Ungültige Feed-Adresse');
	return parsed.toString();
}

async function subscribedId(feedUrl: string): Promise<number | null> {
	const [row] = await db
		.select({ id: itemsTable.id })
		.from(itemsTable)
		.where(and(eq(itemsTable.kind, 'podcast'), eq(itemsTable.feedUrl, feedUrl)));
	return row?.id ?? null;
}

export const load = async ({ locals, url }) => {
	const userId = await requireWebAdmin(locals, db);
	const feedUrl = validFeedUrl(url.searchParams.get('feed'));
	// Carried through from the search so "Zurück zur Suche" returns to the same
	// result list instead of an empty search box.
	const query = url.searchParams.get('q') ?? '';
	const [podcasts, existingId] = await Promise.all([
		podcastOverview(db, userId),
		subscribedId(feedUrl)
	]);
	const unread = podcasts.reduce((sum, p) => sum + (p.unheard_count ?? 0), 0);

	try {
		const feed = await previewFeed(feedUrl);
		return {
			feedUrl,
			query,
			subscribedId: existingId,
			podcasts,
			unread,
			feed: {
				title: feed.title,
				description: feed.description,
				imageUrl: feed.imageUrl,
				episodeCount: feed.episodes.length,
				// The whole feed can run to thousands of items; the preview only has
				// to answer "is this the show I meant?".
				episodes: feed.episodes.slice(0, 30)
			},
			loadError: null
		};
	} catch (e: unknown) {
		if (e instanceof FeedFetchError || e instanceof InvalidFeedError) {
			return {
				feedUrl,
				query,
				subscribedId: existingId,
				podcasts,
				unread,
				feed: null,
				loadError:
					e instanceof FeedFetchError
						? 'Der Feed ist nicht erreichbar.'
						: 'Der Feed konnte nicht gelesen werden.'
			};
		}
		throw e;
	}
};

export const actions = {
	default: async ({ locals, url }) => {
		await requireWebAdmin(locals, db);
		const feedUrl = validFeedUrl(url.searchParams.get('feed'));
		let podcastId: number;
		try {
			const { coverDir } = loadConfig(process.env as Record<string, string | undefined>);
			podcastId = (await subscribe(db, feedUrl, { coversDir: coverDir })).id;
		} catch (e: unknown) {
			if (e instanceof FeedFetchError)
				return fail(502, { error: 'Der Feed ist nicht erreichbar.' });
			if (e instanceof InvalidFeedError)
				return fail(422, { error: 'Der Feed konnte nicht gelesen werden.' });
			throw e;
		}
		// Outside the catch: redirect() signals by throwing, and swallowing it
		// here would turn a successful subscribe into a 500.
		redirect(303, `/library/podcasts/${podcastId}`);
	}
};
