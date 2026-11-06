import { isRedirect } from '@sveltejs/kit';
import createClient from 'openapi-fetch';
import { describe, expect, it } from 'vitest';
import type { paths } from '#lib/api/schema.d.ts';
import { GET } from './+server';

type Event = Parameters<typeof GET>[0];

/** The route, on the real client of the admin API, with `answer` in the server's place. */
function exportWith(answer: () => Response, query = '') {
	const asked: URL[] = [];
	const api = createClient<paths>({
		baseUrl: 'http://server.test',
		fetch: async (request) => {
			asked.push(new URL(request.url));
			return answer();
		}
	});
	const url = new URL(`http://console.test/catalogue/export${query}`);
	return { asked, result: GET({ locals: { api }, url } as unknown as Event) };
}

/** Where the route sent the person instead of answering with a file. */
async function redirected(result: ReturnType<typeof GET>): Promise<string> {
	try {
		await result;
	} catch (thrown) {
		if (isRedirect(thrown)) return thrown.location;
		throw thrown;
	}
	throw new Error('the route answered with a file');
}

describe('the catalogue export', () => {
	// Spaced as no serialiser would write it: the file holds the server's bytes, not a copy.
	const catalogue = '{ "schema" :  "pacod.catalog/v1" }';

	it('answers the server’s bytes as a file', async () => {
		const { asked, result } = exportWith(() => new Response(catalogue));
		const file = await result;
		expect(await file.text()).toBe(catalogue);
		expect(file.headers.get('content-type')).toBe('application/json');
		expect(file.headers.get('content-disposition')).toMatch(
			/^attachment; filename="pacod-catalog-\d{4}-\d{2}-\d{2}\.json"$/
		);
		expect(asked[0].pathname).toBe('/api/v1/catalog');
		expect(asked[0].searchParams.has('vendor')).toBe(false);
	});

	it('asks for the ticked vendors in the one parameter the API takes', async () => {
		const { asked, result } = exportWith(() => new Response(catalogue), '?vendor=a&vendor=b');
		const file = await result;
		expect(asked[0].searchParams.getAll('vendor')).toEqual(['a,b']);
		expect(file.headers.get('content-disposition')).toContain('-partial.json');
	});

	it('sends the person back when the server refuses', async () => {
		const { result } = exportWith(() => Response.json({ error: {} }, { status: 502 }));
		expect(await redirected(result)).toBe('/catalogue?export_failed=1');
	});

	it('sends the person back when the body breaks off, instead of a truncated file', async () => {
		const broken = new ReadableStream({
			start: (stream) => stream.error(new Error('timed out'))
		});
		const { result } = exportWith(() => new Response(broken));
		expect(await redirected(result)).toBe('/catalogue?export_failed=1');
	});
});
