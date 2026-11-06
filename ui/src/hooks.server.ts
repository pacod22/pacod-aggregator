import { redirect } from '@sveltejs/kit';
import { sequence, type Handle } from '@sveltejs/kit/hooks';
import { AGG_CONSOLE_API_URL } from '$app/env/private';
import { allowFormTarget } from '#lib/csp.ts';
import { may } from '#lib/nav.ts';
import { createApi, problemText } from '#lib/server/api.ts';
import { identity } from '#lib/server/identity.ts';

const issuerOrigin = identity ? new URL(identity.issuer).origin : null;

const app: Handle = async ({ event, resolve }) => {
	const { locals, cookies, url } = event;
	const login = () => `/auth/login?next=${encodeURIComponent(url.pathname + url.search)}`;
	locals.token = locals.session?.accessToken ?? null;
	locals.api = createApi(AGG_CONSOLE_API_URL, locals.token);
	locals.principal = null;
	locals.apiProblem = null;

	// pacod-id's own routes come first, in `identity.handle`; these need a session.
	if (!url.pathname.startsWith('/auth/')) {
		if (!locals.token) redirect(303, login());
		// Who is calling, asked once per request.
		const { data, error, response } = await locals.api.GET('/api/v1/session');
		if (response.status === 401) {
			identity?.forget(cookies);
			redirect(303, login());
		}
		const noRole = response.status === 403;
		const onlyCatalogue = response.ok && !may(data?.permissions ?? [], 'agg.data.read');
		if (response.ok && data) locals.principal = data;
		if (noRole || onlyCatalogue) {
			if (url.pathname !== '/no-access') redirect(303, '/no-access');
		} else if (!response.ok) {
			locals.apiProblem = problemText(error, response.status);
		}
	}

	const response = await resolve(event);
	if (issuerOrigin) {
		const policy = allowFormTarget(response.headers.get('content-security-policy'), issuerOrigin);
		if (policy) response.headers.set('content-security-policy', policy);
	}
	if (!response.headers.has('cache-control')) response.headers.set('cache-control', 'no-store');
	response.headers.set('x-content-type-options', 'nosniff');
	response.headers.set('x-frame-options', 'DENY');
	response.headers.set('referrer-policy', 'same-origin');
	return response;
};

/** pacod-id's sign-in routes and the session come first. */
export const handle: Handle = identity ? sequence(identity.handle as unknown as Handle, app) : app;
