<script lang="ts">
	import ThemeToggle from './ThemeToggle.svelte';
	import Icon from './Icon.svelte';

	let {
		accent,
		activeHref,
		user,
		counts
	}: {
		accent: 'book' | 'music' | 'podcast';
		activeHref: string;
		user: { name: string; isAdmin: boolean };
		counts: {
			albums: number;
			artists: number;
			podcasts: number;
			unreadEpisodes: number;
			books: number;
		};
	} = $props();

	const initials = $derived(user.name.slice(0, 2).toUpperCase());
</script>

<nav class="sidebar" style="--a: var(--{accent})">
	<div class="brand">
		<span class="logo-ring"></span>
		<span class="wordmark">Autoreverse</span>
	</div>
	<a href="/library/search" class="search-shortcut">Alles durchsuchen</a>

	<div class="eyebrow" style="color: var(--music)">Musik</div>
	<a href="/library/albums" aria-current={activeHref === '/library/albums'}
		>Alben <span class="count mono">{counts.albums}</span></a
	>
	<a href="/library/artists" aria-current={activeHref === '/library/artists'}
		>Interpreten <span class="count mono">{counts.artists}</span></a
	>
	<a href="/library/playlists" aria-current={activeHref === '/library/playlists'}>Playlists</a>
	<a href="/library/favorites" aria-current={activeHref === '/library/favorites'}>Favoriten</a>

	<div class="eyebrow" style="color: var(--podcast)">Podcasts</div>
	<a href="/library/podcasts" aria-current={activeHref === '/library/podcasts'}>
		Abos <span class="count mono">{counts.podcasts}</span>
	</a>
	<a
		href="/library/podcasts?filter=new"
		aria-current={activeHref === '/library/podcasts?filter=new'}
	>
		Neue Folgen {#if counts.unreadEpisodes > 0}<span class="badge">{counts.unreadEpisodes}</span
			>{/if}
	</a>

	<div class="eyebrow" style="color: var(--book)">Hörbücher</div>
	<a href="/library/books" aria-current={activeHref === '/library/books'}
		>Bibliothek <span class="count mono">{counts.books}</span></a
	>
	<a href="/library/series" aria-current={activeHref === '/library/series'}>Serien</a>
	<a href="/library/bookmarks" aria-current={activeHref === '/library/bookmarks'}>Lesezeichen</a>

	<div class="account">
		<span class="avatar">{initials}</span>
		<span class="who">
			<span class="name">{user.name}</span>
			<span class="role mono">{user.isAdmin ? 'Verwalter' : 'Nutzer'}</span>
		</span>
		<ThemeToggle />
		<a href="/settings" class="settings-link" aria-label="Einstellungen">
			<Icon name="settings" />
		</a>
		<form method="POST" action="/logout" class="logout-form">
			<button type="submit" class="logout-button" aria-label="Abmelden">
				<Icon name="logout" />
			</button>
		</form>
	</div>
</nav>

<style>
	nav {
		width: var(--sidebar-width);
		display: flex;
		flex-direction: column;
		padding: 20px 12px 12px;
		box-sizing: border-box;
		position: sticky;
		top: 0;
		height: 100vh;
		overflow-y: auto;
		background: linear-gradient(
			180deg,
			color-mix(in oklab, var(--a) 22%, var(--sidebar)),
			var(--sidebar) 52%
		);
		border-right: 1px solid var(--line);
	}
	.brand {
		display: flex;
		align-items: center;
		gap: 9px;
		padding: 0 10px 18px;
	}
	.logo-ring {
		width: 15px;
		height: 15px;
		border-radius: 50%;
		border: 3px solid var(--a);
		box-sizing: border-box;
	}
	.wordmark {
		font: 600 15px/1 var(--font-sans);
		letter-spacing: -0.015em;
	}
	.search-shortcut {
		display: block;
		margin: 0 4px 16px;
		padding: 7px 10px;
		border-radius: 8px;
		background: var(--panel);
		color: var(--faint);
		font: 400 12.5px/1 var(--font-sans);
	}
	.eyebrow {
		padding: 20px 10px 8px;
	}
	nav > a {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 7px 10px;
		border-radius: 7px;
		font: 400 13.5px/1.2 var(--font-sans);
		color: var(--dim);
	}
	nav > a:hover {
		background: var(--panel);
		color: var(--text);
	}
	nav > a[aria-current='true'] {
		background: var(--a);
		color: var(--bg);
		font-weight: 500;
	}
	.count {
		margin-left: auto;
		font-size: 11px;
		color: var(--faint);
	}
	.badge {
		margin-left: auto;
	}
	.account {
		margin-top: auto;
		display: flex;
		align-items: center;
		gap: 7px;
		padding: 9px 10px;
		border-radius: 8px;
		border: 1px solid var(--line);
	}
	.avatar {
		width: 24px;
		height: 24px;
		border-radius: 50%;
		background: color-mix(in oklab, var(--a) 30%, transparent);
		display: grid;
		place-items: center;
		font: 600 10px var(--font-sans);
		flex: none;
	}
	.who {
		display: flex;
		flex-direction: column;
		line-height: 1.25;
		overflow: hidden;
		min-width: 0;
	}
	.name,
	.role {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.name {
		font: 500 12px var(--font-sans);
	}
	.role {
		font-size: 10px;
		color: var(--faint);
	}
	.logout-form {
		display: contents;
	}
	.settings-link,
	.logout-button {
		flex: none;
		width: 26px;
		height: 26px;
		display: grid;
		place-items: center;
		border-radius: var(--radius-pill, 999px);
		border: 1px solid var(--line);
		background: transparent;
		color: var(--dim);
	}
	.settings-link:hover,
	.logout-button:hover {
		color: var(--text);
		background: var(--panel);
	}

	@media (max-width: 700px) {
		nav {
			width: 100%;
			flex-direction: row;
			position: fixed;
			top: auto;
			bottom: 0;
			height: 56px;
			overflow: visible;
			padding: 0;
			border-right: none;
			border-top: 1px solid var(--line);
		}
	}
</style>
