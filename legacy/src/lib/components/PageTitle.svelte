<script lang="ts">
	// Without a <title> the browser tab falls back to the bare URL. Every page
	// renders this once with its own name; the brand always trails so the part
	// that identifies the page survives a narrow, truncated tab.
	let { title }: { title?: string } = $props();
</script>

<svelte:head>
	<title>{title ? `${title} · Autoreverse` : 'Autoreverse'}</title>
</svelte:head>
