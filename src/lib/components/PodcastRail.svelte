<script lang="ts">
	type RailPodcast = {
		id: number;
		title: string;
		cover_path: string | null;
		episode_count: number;
		unheard_count: number;
	};

	let {
		podcasts,
		activeId = null,
		children
	}: {
		podcasts: RailPodcast[];
		activeId?: number | null;
		children?: import('svelte').Snippet;
	} = $props();
</script>

<div class="rail">
	<div class="rail-head"><span class="eyebrow">Abos · {podcasts.length}</span></div>
	{#each podcasts as podcast (podcast.id)}
		<a class="entry" href="/library/podcasts/{podcast.id}" aria-current={podcast.id === activeId}>
			<span
				class="cover"
				style={podcast.cover_path ? `background-image: url("/items/${podcast.id}/cover")` : ''}
			></span>
			<span class="meta">
				<span class="name">{podcast.title}</span>
				<span class="sub mono">
					{podcast.episode_count} Folgen{#if podcast.unheard_count > 0}
						· {podcast.unheard_count} neu{/if}
				</span>
			</span>
			{#if podcast.unheard_count > 0}<span class="badge">{podcast.unheard_count}</span>{/if}
		</a>
	{/each}
	{@render children?.()}
</div>

<style>
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
		gap: 11px;
		padding: 8px;
		border-radius: 9px;
		color: inherit;
	}
	.entry:hover,
	.entry[aria-current='true'] {
		background: var(--panel);
	}
	.cover {
		width: 52px;
		height: 52px;
		flex: none;
		border-radius: 8px;
		background: var(--tile) center/cover;
	}
	.meta {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	.name {
		font-size: 13.5px;
		line-height: 1.3;
		display: -webkit-box;
		-webkit-box-orient: vertical;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		overflow: hidden;
	}
	.sub {
		font-size: 10.5px;
		color: var(--faint);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.badge {
		flex: none;
		display: inline-flex;
		align-items: center;
		font: 600 10px var(--font-mono);
		color: var(--bg);
		background: var(--a);
		padding: 2px 6px;
		border-radius: 99px;
	}
</style>
