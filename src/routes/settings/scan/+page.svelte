<script lang="ts">
	import ScanStatusCard from '$lib/components/ScanStatusCard.svelte';
	import PageTitle from '$lib/components/PageTitle.svelte';
</script>

<PageTitle title="Scan" />

<h1>Scan</h1>
<ScanStatusCard />

<style>
	h1 {
		font: 600 20px var(--font-sans);
		margin-bottom: 16px;
	}
</style>
