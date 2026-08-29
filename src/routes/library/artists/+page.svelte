<script lang="ts">
	import { resolve } from '$app/paths';
	import PageTitle from '$lib/components/PageTitle.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();
</script>

<PageTitle title="Interpreten" />

<div class="content" style="--a: var(--music)">
	<h1>Interpreten <span class="count mono">{data.artists.length}</span></h1>
	<div class="grid">
		{#each data.artists as a (a.name)}
			<div class="tile">
				<a class="link" href="{resolve('/library/albums')}?artist={encodeURIComponent(a.name)}">
					<span class="cover">
						{#if a.coverUrl}
							<img src={a.coverUrl} alt="" loading="lazy" />
						{/if}
					</span>
					<span class="name">{a.name}</span>
					<span class="count-sub mono">{a.albumCount} Alben</span>
				</a>
				{#if data.user?.isAdmin}
					<a
						class="edit"
						href={resolve('/library/artists/[name]', { name: a.name })}
						aria-label="Bild für {a.name} wählen"
					>
						Bild
					</a>
				{/if}
			</div>
		{/each}
	</div>
</div>

<style>
	.content {
		padding: 24px 32px;
	}
	h1 {
		font: 600 20px var(--font-sans);
		margin-bottom: 16px;
	}
	.count {
		color: var(--faint);
		font-size: 13px;
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
		gap: 22px 16px;
	}
	.tile {
		position: relative;
		min-width: 0;
	}
	.link {
		display: flex;
		flex-direction: column;
		gap: 8px;
		color: inherit;
		min-width: 0;
	}
	.cover {
		display: block;
		width: 100%;
		aspect-ratio: 1 / 1;
		border-radius: var(--radius-lg);
		background: var(--tile);
		background-image: repeating-linear-gradient(135deg, var(--stripe) 0 1px, transparent 1px 7px);
		overflow: hidden;
	}
	.cover img {
		width: 100%;
		height: 100%;
		object-fit: cover;
		display: block;
	}
	.name {
		font: 500 12.5px/1.3 var(--font-sans);
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
		top: 8px;
		right: 8px;
		padding: 3px 8px;
		border-radius: var(--radius-pill);
		background: rgb(0 0 0 / 0.55);
		color: var(--text);
		font: 500 10px var(--font-sans);
		opacity: 0;
	}
	.tile:hover .edit,
	.edit:focus-visible {
		opacity: 1;
	}
</style>
