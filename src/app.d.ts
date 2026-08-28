declare global {
	namespace App {
		interface Error {
			message: string;
			detail?: string;
		}
		interface Locals {
			userId: number | null;
			token: string | null;
		}
	}
}

export {};
