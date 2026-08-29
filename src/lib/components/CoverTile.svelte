<script lang="ts">
	import Icon from './Icon.svelte';
	let {
		kind,
		coverUrl,
		title,
		subtitle,
		size = 100,
		onPlay,
		playLabel = 'Abspielen'
	}: {
		kind: string;
		coverUrl: string | null;
		title: string;
		subtitle: string;
		size?: number;
		/** Given, the artwork carries a play button — hover or focus reveals it. */
		onPlay?: () => void;
		playLabel?: string;
	} = $props();
</script>

<figure>
	<div class="art">
		{#if coverUrl}
			<img src={coverUrl} alt="" width={size} height={size} />
		{:else}
			<div
				class="placeholder"
				style="--accent: var(--{kind === 'episode' ? 'podcast' : kind})"
			></div>
		{/if}
		{#if onPlay}
			<button
				class="play"
				aria-label={playLabel}
				title={playLabel}
				onclick={(event) => {
					// The tile itself is usually a link to the detail page; playing is its own act.
					event.preventDefault();
					event.stopPropagation();
					onPlay();
				}}
			>
				<Icon name="play-filled" />
			</button>
		{/if}
	</div>
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
	.art {
		position: relative;
		display: block;
		border-radius: var(--radius-md);
	}
	img,
	.placeholder {
		display: block;
		width: 100%;
		height: auto;
		aspect-ratio: 1 / 1;
		border-radius: var(--radius-md);
		object-fit: cover;
	}
	.play {
		position: absolute;
		right: 8px;
		bottom: 8px;
		width: 38px;
		height: 38px;
		padding: 0;
		border: none;
		border-radius: 50%;
		background: var(--a, var(--music));
		color: var(--bg);
		font-size: 19px;
		display: grid;
		place-items: center;
		box-shadow: 0 8px 18px -8px rgb(0 0 0 / 0.8);
		opacity: 0;
		transform: translateY(6px);
		transition:
			opacity 120ms ease,
			transform 120ms ease;
	}
	figure:hover .play,
	.play:focus-visible {
		opacity: 1;
		transform: none;
	}
	@media (hover: none) {
		/* No hover to reveal it on a touch screen. */
		.play {
			opacity: 1;
			transform: none;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.play {
			transition: none;
		}
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
