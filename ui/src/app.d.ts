import type { components } from '#lib/api/schema.d.ts';
import type { ApiClient } from '#lib/server/api.ts';
import type { IdSession } from '@pacod/id-sveltekit';

declare global {
	namespace App {
		interface Locals {
			/** The admin API, with this request's bearer. */
			api: ApiClient;
			token: string | null;
			session?: IdSession | null;
			/** Who is signed in and what they may do, as the server says. */
			principal: components['schemas']['Session'] | null;
			/** Set when the server could not be asked. */
			apiProblem: string | null;
		}
	}
}

export {};
