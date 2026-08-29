<script lang="ts">
	import ContinueCard from '$lib/components/ContinueCard.svelte';
	import CoverTile from '$lib/components/CoverTile.svelte';
	import SegmentFilter from '$lib/components/SegmentFilter.svelte';
	import PageTitle from '$lib/components/PageTitle.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	function greeting(): string {
		const hour = new Date().getHours();
		if (hour < 12) return 'Guten Morgen';
		if (hour < 18) return 'Guten Tag';
		return 'Guten Abend';
	}

	function capitalize(name: string): string {
		return name.charAt(0).toUpperCase() + name.slice(1);
	}

	function detailHref(entry: { kind: string; id: number }): string {
		const segment =
			entry.kind === 'book' ? 'books' : entry.kind === 'album' ? 'albums' : 'podcasts';
		return `/library/${segment}/${entry.id}`;
	}

	function formatDuration(seconds: number): string {
		const hours = seconds / 3600;
		return hours >= 1 ? `${hours.toFixed(1)} h` : `${Math.round(seconds / 60)} min`;
	}
</script>

<PageTitle title="Start" />

<div class="content">
	<header>
		<h1>{greeting()}, {data.user?.name ? capitalize(data.user.name) : ''}</h1>
		<SegmentFilter
			options={[
				{ label: 'Alles', value: 'all' },
				{ label: 'Musik', value: 'album' },
				{ label: 'Podcasts', value: 'podcast' },
				{ label: 'Hörbücher', value: 'book' }
			]}
			value="all"
			onChange={() => {}}
		/>
	</header>

	<section>
		<div class="section-head">
			<h2>Weiter hören</h2>
			<span class="meta mono">{data.continueEntries.length} offen</span>
		</div>
		<div class="grid-3">
			{#each data.continueEntries as entry (entry.id)}
				<ContinueCard
					kind={entry.kind}
					coverUrl={entry.cover_path ? `/items/${entry.id}/cover` : null}
					title={entry.title}
					subtitle={(entry.kind === 'book' ? entry.author : entry.artist) ?? ''}
					progressPercent={entry.duration > 0 ? (entry.position / entry.duration) * 100 : 0}
					durationLabel={formatDuration(entry.duration)}
					href={detailHref(entry)}
				/>
			{/each}
		</div>
	</section>

	<section>
		<div class="section-head">
			<h2>Neu im Bestand</h2>
			<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
			<a href="/library/albums?sort=added">Alle ansehen</a>
		</div>
		<div class="grid-6">
			{#each data.recent as item (item.id)}
				<CoverTile
					kind={item.kind}
					coverUrl={item.coverPath ? `/items/${item.id}/cover` : null}
					title={item.title}
					subtitle={item.artist ?? item.author ?? ''}
				/>
			{/each}
		</div>
	</section>
</div>

<style>
	.content {
		padding: 24px 32px;
		overflow: hidden;
	}
	.section-head {
		display: flex;
		align-items: baseline;
		gap: 10px;
		margin: 24px 0 14px;
	}
	.grid-3 {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
		gap: 14px;
	}
	.grid-6 {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
		gap: 22px 16px;
	}
</style>
