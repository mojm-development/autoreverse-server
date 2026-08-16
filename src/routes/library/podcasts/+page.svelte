<script lang="ts">
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();
	let feedUrl = $state('');

	async function subscribe() {
		await fetch('/podcasts', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ feed_url: feedUrl })
		});
		feedUrl = '';
		location.reload();
	}
</script>

<div class="layout" style="--a: var(--podcast)">
	<div class="rail">
		<div class="rail-head">
			<span class="eyebrow">Abos · {data.podcasts.length}</span>
		</div>
		{#each data.podcasts as podcast (podcast.id)}
			<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
			<a class="entry" href="/library/podcasts/{podcast.id}">
				<span
					class="cover"
					style={podcast.cover_path ? `background-image: url(/items/${podcast.id}/cover)` : ''}
				></span>
				<span class="name">{podcast.title}</span>
				<span class="episode-count mono">{podcast.episode_count}</span>
				{#if podcast.unheard_count > 0}<span class="badge">{podcast.unheard_count}</span>{/if}
			</a>
		{/each}
		{#if data.user?.isAdmin}
			<form
				class="add-feed"
				onsubmit={(e) => {
					e.preventDefault();
					subscribe();
				}}
			>
				<input type="url" bind:value={feedUrl} placeholder="Feed-URL hinzufügen" required />
				<button type="submit">Abonnieren</button>
			</form>
		{/if}
	</div>
	<div class="content">
		<p class="empty">Wähle einen Podcast aus der Liste.</p>
	</div>
</div>

<style>
	.layout {
		display: flex;
		min-height: 100%;
	}
	.rail {
		width: 274px;
		flex: none;
		padding: 20px 14px;
		border-right: 1px solid var(--line);
	}
	.rail-head {
		margin-bottom: 12px;
	}
	.entry {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 7px 8px;
		border-radius: 8px;
		color: inherit;
	}
	.entry:hover {
		background: var(--panel);
	}
	.cover {
		width: 32px;
		height: 32px;
		flex: none;
		border-radius: 6px;
		background: var(--tile);
		background-size: cover;
		background-position: center;
	}
	.name {
		flex: 1;
		min-width: 0;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		font-size: 12.5px;
	}
	.episode-count {
		color: var(--faint);
		font-size: 10px;
	}
	.badge {
		display: inline-flex;
		align-items: center;
		font: 600 10px var(--font-mono);
		color: var(--bg);
		background: var(--a);
		padding: 2px 5px;
		border-radius: 99px;
	}
	.add-feed {
		margin-top: 14px;
		padding: 10px;
		border: 1px dashed var(--line-strong);
		border-radius: 8px;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.add-feed input {
		height: 30px;
		padding: 0 8px;
		border-radius: 6px;
		background: var(--panel);
		border: 1px solid var(--line);
		color: var(--text);
		font-size: 12px;
	}
	.add-feed button {
		height: 28px;
		border-radius: 6px;
		border: none;
		background: var(--a);
		color: var(--bg);
		font: 500 12px var(--font-sans);
	}
	.content {
		flex: 1;
		padding: 24px 32px;
	}
	.empty {
		color: var(--faint);
	}
</style>
