<script lang="ts">
	import { getContext } from 'svelte';
	import { enhance } from '$app/forms';
	import type { SubmitFunction } from '@sveltejs/kit';
	import { PLAYER_CONTEXT_KEY, type PlayerStore, type PlayerPreferences } from '$lib/player.svelte';
	import PageTitle from '$lib/components/PageTitle.svelte';
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();

	const player = getContext<PlayerStore>(PLAYER_CONTEXT_KEY);
	let saved = $state(false);
	let timer: ReturnType<typeof setTimeout> | null = null;

	const submit: SubmitFunction =
		() =>
		async ({ result, update }) => {
			if (result.type === 'success' && result.data?.prefs) {
				player.applyPreferences(result.data.prefs as PlayerPreferences);
				saved = true;
				if (timer) clearTimeout(timer);
				timer = setTimeout(() => (saved = false), 2500);
			}
			await update({ reset: false });
		};
</script>

<PageTitle title="Wiedergabe" />

<h1>Wiedergabe</h1>
<form method="POST" class="form" use:enhance={submit}>
	<label>
		<span class="eyebrow">Geschwindigkeit</span>
		<input
			type="range"
			name="playbackSpeed"
			min={data.bounds.SPEED_MIN}
			max={data.bounds.SPEED_MAX}
			step="0.05"
			value={data.prefs.playbackSpeed}
		/>
	</label>
	<label>
		<span class="eyebrow">Zurückspringen (Sekunden)</span>
		<input
			type="number"
			name="skipBack"
			min={data.bounds.SKIP_MIN}
			max={data.bounds.SKIP_MAX}
			value={data.prefs.skipBack}
		/>
	</label>
	<label>
		<span class="eyebrow">Vorspringen (Sekunden)</span>
		<input
			type="number"
			name="skipForward"
			min={data.bounds.SKIP_MIN}
			max={data.bounds.SKIP_MAX}
			value={data.prefs.skipForward}
		/>
	</label>
	<div class="save">
		<button type="submit" class="primary">Speichern</button>
		<span class="saved" role="status" aria-live="polite">{saved ? 'Gespeichert' : ''}</span>
	</div>
</form>

<style>
	h1 {
		font: 600 20px var(--font-sans);
		margin-bottom: 16px;
	}
	.save {
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.saved {
		color: var(--faint);
		font-size: 12px;
	}
	.form {
		display: flex;
		flex-direction: column;
		gap: 14px;
		max-width: 340px;
	}
	.form input[type='number'] {
		height: 32px;
		padding: 0 10px;
		border-radius: var(--radius-md);
		background: var(--panel);
		border: 1px solid var(--line);
		color: var(--text);
	}
	.primary {
		align-self: flex-start;
		height: 32px;
		padding: 0 16px;
		border-radius: var(--radius-pill);
		background: var(--a);
		color: var(--bg);
		border: none;
	}
</style>
