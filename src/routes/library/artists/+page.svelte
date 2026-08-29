<script lang="ts">
	import { resolve } from '$app/paths';
	import Icon from '$lib/components/Icon.svelte';
	import PageTitle from '$lib/components/PageTitle.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	let query = $state('');
	const needle = $derived(query.trim().toLowerCase());
	const shown = $derived(
		needle ? data.artists.filter((a) => a.name.toLowerCase().includes(needle)) : data.artists
	);

	/** Initials for an artist without a picture — better than an empty tile. */
	function initials(name: string): string {
		return name
			.split(/\s+/)
			.slice(0, 2)
			.map((part) => part[0] ?? '')
			.join('')
			.toUpperCase();
	}
</script>

<PageTitle title="Interpreten" />

<div class="content" style="--a: var(--music)">
	<header>
		<h1>Interpreten <span class="count mono">{data.artists.length}</span></h1>
		<span class="search-field">
			<Icon name="search" />
			<input
				type="search"
				placeholder="Interpret suchen"
				value={query}
				oninput={(e) => (query = e.currentTarget.value)}
			/>
		</span>
	</header>

	{#if shown.length === 0}
		<p class="empty">
			{data.artists.length === 0
				? 'Noch keine Interpreten in der Bibliothek.'
				: `Kein Interpret passt zu „${query}“.`}
		</p>
	{:else}
		<ul class="grid">
			{#each shown as artist (artist.name)}
				<li class="tile">
					<a
						class="link"
						href="{resolve('/library/albums')}?artist={encodeURIComponent(artist.name)}"
					>
						<span class="portrait">
							{#if artist.coverUrl}
								<img src={artist.coverUrl} alt="" loading="lazy" />
							{:else}
								<span class="initials">{initials(artist.name)}</span>
							{/if}
						</span>
						<span class="name">{artist.name}</span>
						<span class="count-sub mono">
							{artist.albumCount}
							{artist.albumCount === 1 ? 'Album' : 'Alben'}
						</span>
					</a>
					{#if data.user?.isAdmin}
						<a
							class="edit"
							href={resolve('/library/artists/[name]', { name: artist.name })}
							aria-label="Bild für {artist.name} wählen"
							title="Bild wählen"
						>
							<Icon name="settings" />
						</a>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</div>

<style>
	.content {
		padding: 24px 32px 32px;
	}
	header {
		display: flex;
		align-items: center;
		gap: 16px;
		flex-wrap: wrap;
		margin-bottom: 20px;
	}
	h1 {
		font: 600 20px var(--font-sans);
		margin: 0;
	}
	.count {
		color: var(--faint);
		font-size: 13px;
	}
	.search-field {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 32px;
		padding: 0 10px;
		border-radius: var(--radius-md);
		background: var(--panel);
		border: 1px solid var(--line);
		color: var(--faint);
		margin-left: auto;
	}
	.search-field:focus-within {
		border-color: var(--line-strong);
		color: var(--dim);
	}
	.search-field input {
		width: 200px;
		border: none;
		background: transparent;
		color: var(--text);
		font: 400 12.5px var(--font-sans);
		outline: none;
	}
	.search-field input::-webkit-search-cancel-button {
		display: none;
	}

	.grid {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(132px, 1fr));
		gap: 24px 16px;
	}
	.tile {
		position: relative;
		min-width: 0;
	}
	.link {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 9px;
		color: inherit;
		min-width: 0;
		text-align: center;
	}
	/* People are round here: a circle tells an artist apart from an album at a glance. */
	.portrait {
		display: grid;
		place-items: center;
		width: 100%;
		aspect-ratio: 1 / 1;
		border-radius: 50%;
		overflow: hidden;
		background: var(--tile);
		background-image: repeating-linear-gradient(135deg, var(--stripe) 0 1px, transparent 1px 7px);
		transition: transform 140ms ease;
	}
	.link:hover .portrait {
		transform: scale(1.03);
	}
	.portrait img {
		width: 100%;
		height: 100%;
		object-fit: cover;
		display: block;
	}
	.initials {
		font: 600 22px var(--font-sans);
		color: var(--faint);
		letter-spacing: 0.04em;
	}
	.name {
		font: 500 12.5px/1.3 var(--font-sans);
		max-width: 100%;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.count-sub {
		color: var(--faint);
		font-size: 11px;
	}
	.edit {
		position: absolute;
		top: 4px;
		right: 4px;
		display: grid;
		place-items: center;
		width: 28px;
		height: 28px;
		border-radius: 50%;
		background: rgb(0 0 0 / 0.55);
		color: #fff;
		font-size: 14px;
		opacity: 0;
	}
	.tile:hover .edit,
	.edit:focus-visible {
		opacity: 1;
	}
	.empty {
		color: var(--faint);
	}

	@media (max-width: 700px) {
		.content {
			padding: 18px 16px 24px;
		}
		.search-field {
			margin-left: 0;
		}
		.search-field input {
			width: 100%;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.portrait {
			transition: none;
		}
		.link:hover .portrait {
			transform: none;
		}
	}
</style>
