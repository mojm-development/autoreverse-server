import {
	pgTable,
	integer,
	text,
	timestamp,
	boolean,
	real,
	doublePrecision,
	primaryKey,
	uniqueIndex,
	index,
	check,
	type AnyPgColumn
} from 'drizzle-orm/pg-core';
import { sql } from 'drizzle-orm';

export const users = pgTable(
	'users',
	{
		id: integer('id').generatedByDefaultAsIdentity().primaryKey(),
		name: text('name').notNull().unique(),
		passwordHash: text('password_hash').notNull(),
		createdAt: timestamp('created_at', { withTimezone: true }).notNull().defaultNow(),
		isAdmin: boolean('is_admin').notNull().default(false),
		playbackSpeed: real('playback_speed').notNull().default(1.0),
		skipBack: integer('skip_back').notNull().default(30),
		skipForward: integer('skip_forward').notNull().default(15)
	},
	(t) => [
		check('playback_speed_range', sql`${t.playbackSpeed} >= 0.5 AND ${t.playbackSpeed} <= 4.0`),
		check('skip_back_range', sql`${t.skipBack} > 0 AND ${t.skipBack} <= 300`),
		check('skip_forward_range', sql`${t.skipForward} > 0 AND ${t.skipForward} <= 300`)
	]
);

export const authTokens = pgTable('auth_tokens', {
	value: text('value').primaryKey(),
	userId: integer('user_id')
		.notNull()
		.references(() => users.id, { onDelete: 'cascade' }),
	createdAt: timestamp('created_at', { withTimezone: true }).notNull().defaultNow(),
	lastSeenAt: timestamp('last_seen_at', { withTimezone: true })
});

export const items = pgTable(
	'items',
	{
		id: integer('id').generatedByDefaultAsIdentity().primaryKey(),
		parentId: integer('parent_id').references((): AnyPgColumn => items.id, { onDelete: 'cascade' }),
		kind: text('kind').notNull(),
		sourcePath: text('source_path').unique(),
		title: text('title').notNull(),
		sortTitle: text('sort_title').notNull(),
		coverPath: text('cover_path'),
		artist: text('artist'),
		albumArtist: text('album_artist'),
		year: integer('year'),
		author: text('author'),
		narrator: text('narrator'),
		series: text('series'),
		seriesIndex: doublePrecision('series_index'),
		feedUrl: text('feed_url'),
		lastChecked: timestamp('last_checked', { withTimezone: true }),
		publishedAt: timestamp('published_at', { withTimezone: true }),
		addedAt: timestamp('added_at', { withTimezone: true }).notNull().defaultNow(),
		missingSince: timestamp('missing_since', { withTimezone: true }),
		guid: text('guid'),
		// Podcasting 2.0 <podcast:chapters url=…>: fetched when the episode is downloaded,
		// since a feed refresh must not pull one JSON file per episode.
		chaptersUrl: text('chapters_url'),
		keepEpisodes: integer('keep_episodes')
	},
	(t) => [
		check('kind_check', sql`${t.kind} IN ('book','album','podcast','episode')`),
		index('item_kind_sort').on(t.kind, t.sortTitle),
		index('item_series')
			.on(t.series)
			.where(sql`${t.series} IS NOT NULL`),
		index('item_parent')
			.on(t.parentId)
			.where(sql`${t.parentId} IS NOT NULL`),
		uniqueIndex('item_guid')
			.on(t.parentId, t.guid)
			.where(sql`${t.guid} IS NOT NULL`)
	]
);

export const tracks = pgTable(
	'tracks',
	{
		id: integer('id').generatedByDefaultAsIdentity().primaryKey(),
		itemId: integer('item_id')
			.notNull()
			.references(() => items.id, { onDelete: 'cascade' }),
		position: integer('position').notNull(),
		path: text('path').notNull().unique(),
		duration: doublePrecision('duration').notNull(),
		title: text('title'),
		disc: integer('disc'),
		mtime: doublePrecision('mtime'),
		size: integer('size')
	},
	(t) => [uniqueIndex('track_item_position').on(t.itemId, t.position)]
);

export const chapters = pgTable(
	'chapters',
	{
		id: integer('id').generatedByDefaultAsIdentity().primaryKey(),
		itemId: integer('item_id')
			.notNull()
			.references(() => items.id, { onDelete: 'cascade' }),
		position: integer('position').notNull(),
		title: text('title').notNull(),
		start: doublePrecision('start').notNull(),
		end: doublePrecision('end').notNull()
	},
	(t) => [uniqueIndex('chapter_item_position').on(t.itemId, t.position)]
);

export const progress = pgTable(
	'progress',
	{
		userId: integer('user_id')
			.notNull()
			.references(() => users.id, { onDelete: 'cascade' }),
		itemId: integer('item_id')
			.notNull()
			.references(() => items.id, { onDelete: 'cascade' }),
		position: doublePrecision('position').notNull(),
		finished: boolean('finished').notNull().default(false),
		updatedAt: timestamp('updated_at', { withTimezone: true }).notNull().defaultNow()
	},
	(t) => [primaryKey({ columns: [t.userId, t.itemId] })]
);

export const bookmarks = pgTable(
	'bookmarks',
	{
		id: integer('id').generatedByDefaultAsIdentity().primaryKey(),
		userId: integer('user_id')
			.notNull()
			.references(() => users.id, { onDelete: 'cascade' }),
		itemId: integer('item_id')
			.notNull()
			.references(() => items.id, { onDelete: 'cascade' }),
		position: doublePrecision('position').notNull(),
		title: text('title').notNull()
	},
	(t) => [uniqueIndex('bookmark_user_item_position').on(t.userId, t.itemId, t.position)]
);

export const favorites = pgTable(
	'favorites',
	{
		userId: integer('user_id')
			.notNull()
			.references(() => users.id, { onDelete: 'cascade' }),
		itemId: integer('item_id').references(() => items.id, { onDelete: 'cascade' }),
		trackId: integer('track_id').references(() => tracks.id, { onDelete: 'cascade' })
	},
	(t) => [
		check('favorite_xor', sql`(${t.itemId} IS NULL) <> (${t.trackId} IS NULL)`),
		uniqueIndex('favorite_item')
			.on(t.userId, t.itemId)
			.where(sql`${t.itemId} IS NOT NULL`),
		uniqueIndex('favorite_track')
			.on(t.userId, t.trackId)
			.where(sql`${t.trackId} IS NOT NULL`)
	]
);

export const listenEvents = pgTable('listen_events', {
	id: integer('id').generatedByDefaultAsIdentity().primaryKey(),
	userId: integer('user_id')
		.notNull()
		.references(() => users.id, { onDelete: 'cascade' }),
	itemId: integer('item_id')
		.notNull()
		.references(() => items.id, { onDelete: 'cascade' }),
	seconds: doublePrecision('seconds').notNull(),
	at: timestamp('at', { withTimezone: true }).notNull().defaultNow()
});

export const playbackSessions = pgTable('playback_sessions', {
	id: text('id').primaryKey(),
	userId: integer('user_id')
		.notNull()
		.references(() => users.id, { onDelete: 'cascade' }),
	itemId: integer('item_id')
		.notNull()
		.references(() => items.id, { onDelete: 'cascade' }),
	startedAt: timestamp('started_at', { withTimezone: true }).notNull().defaultNow(),
	startPosition: doublePrecision('start_position').notNull().default(0),
	closedAt: timestamp('closed_at', { withTimezone: true })
});

export const playlists = pgTable(
	'playlists',
	{
		id: integer('id').generatedByDefaultAsIdentity().primaryKey(),
		userId: integer('user_id')
			.notNull()
			.references(() => users.id, { onDelete: 'cascade' }),
		name: text('name').notNull(),
		createdAt: timestamp('created_at', { withTimezone: true }).notNull().defaultNow()
	},
	(t) => [index('playlist_user').on(t.userId)]
);

export const libraryConfig = pgTable(
	'library_config',
	{
		id: integer('id').primaryKey(),
		booksDir: text('books_dir'),
		musicDir: text('music_dir'),
		podcastKeepEpisodes: integer('podcast_keep_episodes').notNull().default(0),
		updatedAt: timestamp('updated_at', { withTimezone: true }).notNull().defaultNow()
	},
	(t) => [check('keep_episodes_range', sql`${t.podcastKeepEpisodes} BETWEEN 0 AND 50`)]
);

export const artistCovers = pgTable(
	'artist_covers',
	{
		artist: text('artist').primaryKey(),
		itemId: integer('item_id').references(() => items.id, { onDelete: 'cascade' }),
		imagePath: text('image_path'),
		updatedAt: timestamp('updated_at', { withTimezone: true }).notNull().defaultNow()
	},
	(t) => [check('artist_cover_xor', sql`(${t.itemId} IS NULL) <> (${t.imagePath} IS NULL)`)]
);

export const playlistEntries = pgTable(
	'playlist_entries',
	{
		id: integer('id').generatedByDefaultAsIdentity().primaryKey(),
		playlistId: integer('playlist_id')
			.notNull()
			.references(() => playlists.id, { onDelete: 'cascade' }),
		itemId: integer('item_id').references(() => items.id, { onDelete: 'cascade' }),
		trackId: integer('track_id').references(() => tracks.id, { onDelete: 'cascade' }),
		position: integer('position').notNull()
	},
	(t) => [
		check('playlist_entry_xor', sql`(${t.itemId} IS NULL) <> (${t.trackId} IS NULL)`),
		uniqueIndex('playlist_entry_position').on(t.playlistId, t.position)
	]
);
