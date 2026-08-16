<script lang="ts">
	import type { PageData } from './$types';
	let { data }: { data: PageData } = $props();
</script>

<h1>Wiedergabe</h1>
<form method="POST" class="form">
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
	<button type="submit" class="primary">Speichern</button>
</form>

<style>
	h1 {
		font: 600 20px var(--font-sans);
		margin-bottom: 16px;
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
