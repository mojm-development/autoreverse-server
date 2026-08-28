<script lang="ts">
	import logo from '$lib/assets/logo.png';
	let { size = 20 }: { size?: number } = $props();
</script>

<img src={logo} alt="" width={size} height={size} class="mark" />

<style>
	.mark {
		display: block;
		flex: none;
		border-radius: 23%;
	}
</style>
