<script lang="ts">
	let {
		kind,
		coverUrl,
		title,
		subtitle,
		size = 100
	}: {
		kind: string;
		coverUrl: string | null;
		title: string;
		subtitle: string;
		size?: number;
	} = $props();
</script>

<figure>
	{#if coverUrl}
		<img src={coverUrl} alt="" width={size} height={size} />
	{:else}
		<div class="placeholder" style="--accent: var(--{kind === 'episode' ? 'podcast' : kind})"></div>
	{/if}
	<figcaption>
		<span class="title">{title}</span>
		<span class="subtitle">{subtitle}</span>
	</figcaption>
</figure>

<style>
	figure {
		margin: 0;
		display: flex;
		flex-direction: column;
		gap: 8px;
		min-width: 0;
	}
	figcaption {
		min-width: 0;
	}
	img,
	.placeholder {
		width: 100%;
		height: auto;
		aspect-ratio: 1 / 1;
		border-radius: var(--radius-md);
		object-fit: cover;
	}
	.placeholder {
		background: var(--tile);
		background-image: repeating-linear-gradient(135deg, var(--stripe) 0 1px, transparent 1px 7px);
	}
	.title {
		display: block;
		font: 500 12.5px/1.3 var(--font-sans);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.subtitle {
		display: block;
		font: 400 11px/1.2 var(--font-sans);
		color: var(--dim);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
</style>
