<script lang="ts">
	let {
		kind,
		coverUrl,
		title,
		subtitle,
		progressPercent,
		durationLabel,
		href
	}: {
		kind: string;
		coverUrl: string | null;
		title: string;
		subtitle: string;
		progressPercent: number;
		durationLabel: string;
		href: string;
	} = $props();

	// $derived (not a plain const) so `accent` tracks live updates to the
	// `kind` prop rather than capturing only its initial value.
	const accent = $derived(kind === 'episode' ? 'podcast' : kind);
</script>

<a {href} class="card">
	<div class="art">
		{#if coverUrl}
			<img src={coverUrl} alt="" width="74" height="74" />
		{:else}
			<div class="placeholder" style="--accent: var(--{accent})"></div>
		{/if}
	</div>
	<div class="meta">
		<span class="eyebrow" style="color: var(--{accent})">{kind}</span>
		<span class="title">{title}</span>
		<span class="subtitle">{subtitle}</span>
		<div class="progress"><div class="fill" style="width: {progressPercent}%"></div></div>
		<span class="duration mono">{durationLabel}</span>
	</div>
</a>

<style>
	.card {
		display: flex;
		gap: 10px;
		padding: 8px;
		border-radius: var(--radius-md);
		color: inherit;
	}
	.card:hover {
		background: var(--panel);
	}
	.art {
		flex: none;
	}
	img,
	.placeholder {
		width: 74px;
		height: 74px;
		border-radius: var(--radius-md);
		object-fit: cover;
		display: block;
	}
	.placeholder {
		background: var(--tile);
		background-image: repeating-linear-gradient(135deg, var(--stripe) 0 1px, transparent 1px 7px);
	}
	.meta {
		display: flex;
		flex-direction: column;
		gap: 3px;
		min-width: 0;
		justify-content: center;
	}
	.title {
		font: 500 13px/1.3 var(--font-sans);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.subtitle {
		font: 400 11.5px/1.2 var(--font-sans);
		color: var(--dim);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.progress {
		height: 3px;
		border-radius: var(--radius-pill);
		background: var(--track);
		margin-top: 4px;
	}
	.fill {
		height: 100%;
		border-radius: var(--radius-pill);
		background: var(--faint);
	}
	.duration {
		font-size: 10.5px;
		color: var(--faint);
	}
</style>
