<script lang="ts">
	import PageTitle from '$lib/components/PageTitle.svelte';
	import type { ActionData } from './$types';
	let { form }: { form: ActionData } = $props();
</script>

<PageTitle title="Sicherheit" />

<h1>Sicherheit</h1>
<form method="POST" class="form">
	<label>
		<span class="eyebrow">Aktuelles Passwort</span>
		<input name="currentPassword" type="password" required />
	</label>
	<label>
		<span class="eyebrow">Neues Passwort</span>
		<input name="newPassword" type="password" required minlength="8" />
	</label>
	<button type="submit" class="primary">Ändern</button>
	{#if form?.error}<p class="error">{form.error}</p>{/if}
	{#if form?.success}<p class="success">Passwort geändert.</p>{/if}
</form>

<style>
	h1 {
		font: 600 20px var(--font-sans);
		margin-bottom: 16px;
	}
	.form {
		display: flex;
		flex-direction: column;
		gap: 10px;
		max-width: 320px;
	}
	.form input {
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
	.error {
		color: var(--music);
		font-size: 12px;
	}
	.success {
		color: var(--book);
		font-size: 12px;
	}
</style>
