<script lang="ts">
	import type { PageData, ActionData } from './$types';
	let { data, form }: { data: PageData; form: ActionData } = $props();
	let showCreate = $state(false);
</script>

<h1>Nutzer <span class="count mono">{data.users.length}</span></h1>
<button onclick={() => (showCreate = !showCreate)}>Nutzer anlegen</button>

{#if showCreate}
	<form method="POST" action="?/createUser" class="create-form">
		<input name="name" placeholder="Name" required />
		<input name="password" type="password" placeholder="Passwort" required minlength="8" />
		<button type="submit">Anlegen</button>
		{#if form?.error}<p class="error">{form.error}</p>{/if}
	</form>
{/if}

<table>
	<thead><tr><th>Konto</th><th>Rolle</th><th>Zuletzt gesehen</th><th></th></tr></thead>
	<tbody>
		{#each data.users as u (u.id)}
			<tr>
				<td>{u.name}</td>
				<td>{u.isAdmin ? 'Verwalter' : 'Nutzer'}</td>
				<td>{u.lastSeenAt ? new Date(u.lastSeenAt).toLocaleDateString('de-DE') : '—'}</td>
				<td>
					<form method="POST" action="?/toggleAdmin">
						<input type="hidden" name="userId" value={u.id} />
						<input type="hidden" name="isAdmin" value={(!u.isAdmin).toString()} />
						<button type="submit"
							>{u.isAdmin ? 'Verwalter entfernen' : 'Zu Verwalter machen'}</button
						>
					</form>
				</td>
			</tr>
		{/each}
	</tbody>
</table>

<style>
	h1 {
		font: 600 20px var(--font-sans);
		margin-bottom: 10px;
	}
	.count {
		color: var(--faint);
		font-size: 13px;
	}
	.create-form {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
		margin: 10px 0;
	}
	.create-form input {
		flex: 1 1 140px;
		min-width: 0;
		height: 30px;
		padding: 0 8px;
		border-radius: 6px;
		background: var(--panel);
		border: 1px solid var(--line);
		color: var(--text);
	}
	.error {
		color: var(--music);
		font-size: 12px;
	}
	table {
		width: 100%;
		margin-top: 10px;
		border-collapse: collapse;
	}
	th {
		text-align: left;
		color: var(--faint);
		font-size: 11px;
		text-transform: uppercase;
		padding: 6px;
	}
	td {
		padding: 8px 6px;
		border-top: 1px solid var(--line);
		font-size: 12.5px;
	}
</style>
