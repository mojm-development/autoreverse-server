<script lang="ts">
	import { enhance } from '$app/forms';
	import { resolve } from '$app/paths';
	import type { PageData, ActionData } from './$types';
	let { data, form }: { data: PageData; form: ActionData } = $props();
</script>

<div class="content" style="--a: var(--music)">
	<header>
		<h1>{data.artist}</h1>
		<a class="back" href={resolve('/library/artists')}>Zurück zu den Interpreten</a>
	</header>

	{#if form?.error}
		<p class="error" role="alert">{form.error}</p>
	{/if}

	<section>
		<h2>Eigenes Bild</h2>
		<div class="custom">
			<span class="preview">
				{#if data.customImageUrl}
					<img src={data.customImageUrl} alt="" />
				{/if}
			</span>
			<form method="POST" action="?/upload" enctype="multipart/form-data" use:enhance>
				<input type="file" name="image" accept="image/jpeg,image/png,image/webp,image/gif" />
				<button type="submit" class="primary">Hochladen</button>
			</form>
		</div>
		<p class="hint">JPEG, PNG, WebP oder GIF, höchstens 5 MB.</p>
	</section>

	<section>
		<div class="section-head">
			<h2>Album als Bild</h2>
			<form method="POST" action="?/reset" use:enhance>
				<button type="submit" class="secondary">Auf Zufall zurücksetzen</button>
			</form>
		</div>
		{#if data.albums.length === 0}
			<p class="hint">Für diesen Interpreten ist kein Album in der Bibliothek.</p>
		{:else}
			<div class="grid">
				{#each data.albums as album (album.id)}
					{@const active = !data.hasCustomImage && album.id === data.effectiveItemId}
					<form method="POST" action="?/select" use:enhance>
						<input type="hidden" name="item_id" value={album.id} />
						<button type="submit" class="album bare" aria-pressed={active}>
							<span class="cover">
								{#if album.coverUrl}
									<img src={album.coverUrl} alt="" loading="lazy" />
								{/if}
							</span>
							<span class="title">{album.title}</span>
							<span class="year mono">
								{album.year ?? ''}{#if active}
									· gewählt{:else if album.id === data.selectedItemId}
									· gespeichert{/if}
							</span>
						</button>
					</form>
				{/each}
			</div>
		{/if}
	</section>
</div>

<style>
	.content {
		padding: 24px 32px;
	}
	header {
		display: flex;
		align-items: baseline;
		gap: 14px;
		margin-bottom: 18px;
	}
	h1 {
		font: 600 20px var(--font-sans);
		margin: 0;
	}
	.back {
		color: var(--dim);
		font-size: 12.5px;
	}
	h2 {
		font: 600 13px var(--font-sans);
		color: var(--dim);
		margin: 0 0 10px;
	}
	section {
		margin-bottom: 28px;
	}
	.section-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		margin-bottom: 10px;
	}
	.section-head h2 {
		margin: 0;
	}
	.custom {
		display: flex;
		align-items: center;
		gap: 16px;
	}
	.preview {
		width: 96px;
		height: 96px;
		flex: none;
		border-radius: var(--radius-lg);
		background: var(--tile);
		background-image: repeating-linear-gradient(135deg, var(--stripe) 0 1px, transparent 1px 7px);
		overflow: hidden;
		display: block;
	}
	.preview img {
		width: 100%;
		height: 100%;
		object-fit: cover;
		display: block;
	}
	.custom form {
		display: flex;
		align-items: center;
		gap: 10px;
		flex-wrap: wrap;
	}
	.hint {
		color: var(--faint);
		font-size: 11.5px;
		margin: 10px 0 0;
	}
	.error {
		color: var(--music);
		font-size: 12.5px;
		margin: 0 0 14px;
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
		gap: 22px 16px;
	}
	.album {
		display: flex;
		flex-direction: column;
		gap: 8px;
		width: 100%;
		min-width: 0;
	}
	.cover {
		display: block;
		width: 100%;
		aspect-ratio: 1 / 1;
		border-radius: var(--radius-md);
		background: var(--tile);
		background-image: repeating-linear-gradient(135deg, var(--stripe) 0 1px, transparent 1px 7px);
		overflow: hidden;
		border: 2px solid transparent;
	}
	.album[aria-pressed='true'] .cover {
		border-color: var(--a);
	}
	.cover img {
		width: 100%;
		height: 100%;
		object-fit: cover;
		display: block;
	}
	.title {
		font: 500 12.5px/1.3 var(--font-sans);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.year {
		color: var(--faint);
		font-size: 11px;
	}
	.album[aria-pressed='true'] .year {
		color: var(--a);
	}
</style>
