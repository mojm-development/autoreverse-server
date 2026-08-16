<script lang="ts">
	import type { ActionData, PageData } from './$types';
	let { form, data }: { form: ActionData; data: PageData } = $props();
	let showPassword = $state(false);
</script>

<div class="login" style="--a: var(--book)">
	<div class="branding">
		<div class="brand">
			<span class="logo-ring"></span><span class="wordmark">Autoreverse</span>
		</div>
		<div>
			<h1>Ein Server. Alles zum Hören.</h1>
			<p>
				Hörbücher, Podcast-Folgen und Alben in einer Datenbank — eine Suche, ein Fortschritt, eine
				Warteschlange.
			</p>
			<div class="stats">
				<span
					><strong class="mono">{data.counts.book_count}</strong><small class="eyebrow"
						>Hörbücher</small
					></span
				>
				<span
					><strong class="mono">{data.counts.album_count}</strong><small class="eyebrow"
						>Alben</small
					></span
				>
				<span
					><strong class="mono">{data.counts.podcast_count}</strong><small class="eyebrow"
						>Abos</small
					></span
				>
			</div>
		</div>
		<span class="version mono">autoreverse 0.1.0 · {data.hostname}</span>
	</div>
	<div class="form-panel">
		<form method="POST">
			<h2>Anmelden</h2>
			<label>
				<span class="eyebrow">Name</span>
				<input name="name" required autocomplete="username" />
			</label>
			<label>
				<span class="eyebrow">Passwort</span>
				<span class="password-field">
					<input
						name="password"
						type={showPassword ? 'text' : 'password'}
						required
						autocomplete="current-password"
						aria-label="Passwort"
					/>
					<button type="button" onclick={() => (showPassword = !showPassword)}
						>{showPassword ? 'verbergen' : 'zeigen'}</button
					>
				</span>
			</label>
			{#if form?.message}<p role="alert">{form.message}</p>{/if}
			<button type="submit" class="primary">Anmelden</button>
			<div class="warning">
				Diese Verbindung ist unverschlüsselt. Nur im eigenen Netz anmelden oder einen TLS-Proxy
				davorsetzen.
			</div>
		</form>
	</div>
</div>

<style>
	.login {
		display: grid;
		grid-template-columns: 1fr 1fr;
		min-height: 100vh;
	}
	.branding {
		background: linear-gradient(
			160deg,
			color-mix(in oklab, var(--a) 26%, var(--sidebar)),
			var(--sidebar) 62%
		);
		display: flex;
		flex-direction: column;
		justify-content: space-between;
		padding: 52px 48px;
	}
	.brand {
		display: flex;
		align-items: center;
		gap: 11px;
	}
	.logo-ring {
		width: 19px;
		height: 19px;
		border-radius: 50%;
		border: 4px solid var(--a);
	}
	.wordmark {
		font: 600 19px/1 var(--font-sans);
	}
	h1 {
		font: 600 38px/1.15 var(--font-sans);
		letter-spacing: -0.03em;
		max-width: 12ch;
	}
	.branding p {
		color: var(--dim);
		max-width: 34ch;
		line-height: 1.55;
	}
	.stats {
		display: flex;
		gap: 26px;
		margin-top: 34px;
	}
	.stats strong {
		display: block;
		font-size: 20px;
	}
	.form-panel {
		display: flex;
		align-items: center;
		justify-content: center;
	}
	form {
		width: 100%;
		max-width: 360px;
		display: flex;
		flex-direction: column;
	}
	.password-field {
		display: flex;
		align-items: center;
		gap: 6px;
	}
	.password-field input {
		flex: 1;
	}
	.password-field button {
		flex-shrink: 0;
		border: none;
		background: none;
		padding: 4px 2px;
		font: 500 12px/1 var(--font-sans);
		color: var(--dim);
	}
	.password-field button:hover {
		color: var(--text);
	}
	.warning {
		margin-top: 30px;
		padding: 12px 14px;
		border-radius: 10px;
		border: 1px solid color-mix(in oklab, var(--music) 32%, transparent);
		background: color-mix(in oklab, var(--music) 8%, transparent);
		color: var(--dim);
		font-size: 11.5px;
	}
</style>
