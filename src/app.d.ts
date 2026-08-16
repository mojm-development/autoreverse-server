// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces
declare global {
	namespace App {
		// Ruling 8: every error path emits `detail`, including framework-level
		// ones (see hooks.server.ts's handleError). `message` stays required
		// (SvelteKit's default) so the existing page-load `error(status, 'msg')`
		// string-shorthand calls across the app keep type-checking.
		interface Error {
			message: string;
			detail?: string;
		}
		interface Locals {
			userId: number | null;
			token: string | null;
		}
		// interface PageData {}
		// interface PageState {}
		// interface Platform {}
	}
}

export {};
