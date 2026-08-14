import { page } from 'vitest/browser';
import { describe, it, expect } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Sidebar from './Sidebar.svelte';

describe('Sidebar.svelte', () => {
	it('renders the account footer with name and role', async () => {
		render(Sidebar, {
			accent: 'book',
			activeHref: '/library',
			user: { name: 'oliver', isAdmin: true },
			counts: { albums: 1284, artists: 412, podcasts: 14, unreadEpisodes: 7, books: 96 }
		});
		await expect.element(page.getByText('oliver')).toBeInTheDocument();
		await expect.element(page.getByText('Verwalter')).toBeInTheDocument();
	});

	it('shows a "Nutzer" role label for a non-admin', async () => {
		render(Sidebar, {
			accent: 'music',
			activeHref: '/library',
			user: { name: 'mara', isAdmin: false },
			counts: { albums: 0, artists: 0, podcasts: 0, unreadEpisodes: 0, books: 0 }
		});
		await expect.element(page.getByText('Nutzer')).toBeInTheDocument();
	});

	it('renders the unread-episode badge only when > 0', async () => {
		render(Sidebar, {
			accent: 'podcast',
			activeHref: '/library',
			user: { name: 'oliver', isAdmin: true },
			counts: { albums: 0, artists: 0, podcasts: 14, unreadEpisodes: 7, books: 0 }
		});
		await expect.element(page.getByText('7')).toBeInTheDocument();
	});
});
