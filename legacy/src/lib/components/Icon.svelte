<script lang="ts">
	let { name }: { name: string } = $props();
</script>

<svg
	class="icon"
	viewBox="0 0 24 24"
	aria-hidden="true"
	focusable="false"
	data-testid="icon-{name}"
>
	<use href="#icon-{name}"></use>
</svg>

<style>
	.icon {
		width: 1em;
		height: 1em;
		fill: none;
		stroke: currentColor;
		stroke-width: 1.6;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
</style>
