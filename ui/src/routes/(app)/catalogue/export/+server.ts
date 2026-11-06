import { redirect } from '@sveltejs/kit';
import { exportFileName } from '#lib/catalogue.ts';
import type { ApiClient } from '#lib/server/api.ts';
import type { RequestHandler } from './$types';

/**
 * The bytes the server answered, or nothing. Read whole before anything is sent: a failure part of
 * the way through then sends the person back to the catalogue, where a stream would already have
 * promised a file and left them a truncated one. A timeout while reading the body throws, which
 * `createApi`'s fetch cannot catch.
 */
async function catalogue(api: ApiClient, vendor: string | undefined): Promise<ArrayBuffer | null> {
	try {
		const { data, response } = await api.GET('/api/v1/catalog', {
			params: { query: { vendor } },
			parseAs: 'arrayBuffer'
		});
		return response.ok && data ? data : null;
	} catch {
		return null;
	}
}

/** The catalogue as a file. Its bytes are not read here, so it says what `GET /v1/catalog` says. */
export const GET: RequestHandler = async ({ locals, url }) => {
	// Ticked boxes arrive as one parameter each; the API takes them comma separated.
	const vendors = url.searchParams.getAll('vendor').filter(Boolean);
	const body = await catalogue(locals.api, vendors.join(',') || undefined);
	if (!body) redirect(303, '/catalogue?export_failed=1');
	const name = exportFileName(new Date(), vendors.length > 0);
	return new Response(body, {
		headers: {
			'content-type': 'application/json',
			'content-disposition': `attachment; filename="${name}"`
		}
	});
};
