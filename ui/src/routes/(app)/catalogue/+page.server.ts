import { read } from '#lib/server/read.ts';
import type { PageServerLoad } from './$types';

const PAGE = 100;

export const load: PageServerLoad = async ({ locals, url }) => {
	const vendor = url.searchParams.get('vendor') || undefined;
	const status = url.searchParams.get('status') || undefined;
	const cursor = url.searchParams.get('cursor') || undefined;
	const [vendors, models] = await Promise.all([
		read(locals.api.GET('/api/v1/vendors')),
		read(
			locals.api.GET('/api/v1/models', {
				params: { query: { vendor, status, cursor, limit: PAGE } }
			})
		)
	]);
	return { vendors, models, vendor, status, exportFailed: url.searchParams.has('export_failed') };
};
