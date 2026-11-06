/**
 * The typed client of the admin API, generated from docs/openapi.json (`npm run api:types`). It
 * exists on the server only: one per request, with that request's bearer.
 */
import createClient, { type Client } from 'openapi-fetch';
import type { paths } from '#lib/api/schema.d.ts';

export type ApiClient = Client<paths>;

const TIMEOUT_MS = 15_000;

/** A `fetch` that answers instead of throwing when the server cannot be reached. */
async function fetchOrExplain(request: Request): Promise<Response> {
	try {
		return await fetch(request, {
			signal: AbortSignal.any([request.signal, AbortSignal.timeout(TIMEOUT_MS)])
		});
	} catch {
		return Response.json(
			{ error: { type: 'unreachable', message: 'the aggregator did not answer' } },
			{ status: 502 }
		);
	}
}

export function createApi(origin: string, token: string | null): ApiClient {
	return createClient<paths>({
		baseUrl: origin,
		headers: token ? { authorization: `Bearer ${token}` } : {},
		fetch: fetchOrExplain
	});
}

/** The problem an answer carries, as one sentence. The server's errors have this one shape. */
export function problemText(error: unknown, status: number): string {
	const message = (error as { error?: { message?: string } } | undefined)?.error?.message;
	return message ?? `the aggregator answered ${status}`;
}
