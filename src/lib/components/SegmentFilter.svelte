<script lang="ts">
	let {
		options,
		value,
		onChange
	}: { options: { label: string; value: string }[]; value: string; onChange: (v: string) => void } =
		$props();
</script>

<div class="segment" role="tablist">
	{#each options as option (option.value)}
		<button
			type="button"
			role="tab"
			class="pill"
			aria-current={option.value === value}
			onclick={() => onChange(option.value)}
		>
			{option.label}
		</button>
	{/each}
</div>

<style>
	.segment {
		display: flex;
		gap: 6px;
	}
</style>
