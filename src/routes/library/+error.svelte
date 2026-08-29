<script lang="ts">
	import { page } from '$app/state';
	import PageTitle from '$lib/components/PageTitle.svelte';
</script>

<PageTitle title={`Fehler ${page.status}`} />

<div class="error-page" style="--a: var(--book)">
	<span class="code mono">{page.status}</span>
	<h1>{page.error?.message ?? 'Etwas ist schiefgelaufen.'}</h1>
	<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
	<a class="primary" href="/library">Zur Bibliothek</a>
</div>

<style>
	.error-page {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 14px;
		padding: 64px 48px;
	}
	.code {
		font-size: 13px;
		color: var(--faint);
	}
	h1 {
		font: 600 26px/1.3 var(--font-sans);
		margin: 0;
		max-width: 34ch;
	}
	.primary {
		display: inline-flex;
		align-items: center;
		height: 32px;
		padding: 0 16px;
		border-radius: var(--radius-pill);
		background: var(--a, var(--book));
		color: var(--bg);
		font: 500 12px/1 var(--font-sans);
		margin-top: 6px;
	}
</style>
