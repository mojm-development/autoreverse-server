<script lang="ts">
	import { page } from '$app/state';
	import BrandMark from '$lib/components/BrandMark.svelte';
</script>

<div class="error-page">
	<div class="brand"><BrandMark size={24} /><span class="wordmark">Autoreverse</span></div>
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
		max-width: 480px;
		margin: 0 auto;
		padding: 96px 24px;
	}
	.brand {
		display: flex;
		align-items: center;
		gap: 11px;
		margin-bottom: 20px;
	}
	.wordmark {
		font: 600 19px/1 var(--font-sans);
	}
	.code {
		font-size: 13px;
		color: var(--faint);
	}
	h1 {
		font: 600 26px/1.3 var(--font-sans);
		margin: 0;
	}
	.primary {
		display: inline-flex;
		align-items: center;
		height: 32px;
		padding: 0 16px;
		border-radius: var(--radius-pill);
		background: var(--book);
		color: var(--bg);
		font: 500 12px/1 var(--font-sans);
		margin-top: 6px;
	}
</style>
