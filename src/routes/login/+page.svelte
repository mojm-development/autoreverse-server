<script lang="ts">
	import type { ActionData } from './$types';
	let { form }: { form: ActionData } = $props();
</script>

<form method="POST">
	<label>Name <input name="name" required /></label>
	<label>Passwort <input name="password" type="password" required /></label>
	{#if form?.message}<p role="alert">{form.message}</p>{/if}
	<button type="submit">Anmelden</button>
</form>
