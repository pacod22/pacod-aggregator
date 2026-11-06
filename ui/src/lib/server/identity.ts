/**
 * Sign-in through pacod-id, as pacod-gw's console does it (ui/src/lib/server/identity.ts at
 * 50fe4b7). The console is a confidential client of pacod-id (a BFF): `@pacod/id-sveltekit` serves
 * /auth/login, /auth/callback and /auth/logout and keeps pacod-id's tokens sealed in one cookie;
 * the session's access token is the bearer the admin API takes.
 *
 * The console takes no back-channel logout: a session ended at pacod-id is refused by the server
 * within seconds (its copy of pacod-id's feed), and the console then forgets the cookie.
 */
import {
	AGG_CONSOLE_URL,
	AGG_ID_INTERNAL_URL,
	AGG_ID_ISSUER,
	AGG_UI_CLIENT_ID,
	AGG_UI_CLIENT_SECRET,
	AGG_UI_SESSION_SECRET
} from '$app/env/private';
import { building } from '$app/env';
import type { Cookies } from '@sveltejs/kit';
import { createIdHandle, sealedCookie } from '@pacod/id-sveltekit';

/** The cookie's name; `__Host-` is put in front of it on https. */
const COOKIE = 'agg_session';

function required(name: string, value: string | undefined): string {
	if (!value) throw new Error(`${name} is required: the console signs people in through pacod-id`);
	return value;
}

function start() {
	const issuer = required('AGG_ID_ISSUER', AGG_ID_ISSUER);
	const origin = required('AGG_CONSOLE_URL', AGG_CONSOLE_URL);
	const sessionSecret = required('AGG_UI_SESSION_SECRET', AGG_UI_SESSION_SECRET);
	if (sessionSecret.length < 32)
		throw new Error('AGG_UI_SESSION_SECRET must have at least 32 characters');
	const handle = createIdHandle({
		issuer,
		internalUrl: AGG_ID_INTERNAL_URL ?? issuer,
		clientId: AGG_UI_CLIENT_ID,
		clientSecret: required('AGG_UI_CLIENT_SECRET', AGG_UI_CLIENT_SECRET),
		redirectUri: `${origin}/auth/callback`,
		postLogoutRedirectUri: `${origin}/`,
		origin,
		store: sealedCookie({ name: COOKIE, secret: sessionSecret }),
		secret: sessionSecret
	});
	const secure = origin.startsWith('https:');
	return {
		issuer,
		handle,
		account: `${issuer}/account`,
		/** Forgets the session in this browser (the server refused its token). */
		forget(cookies: Cookies): void {
			cookies.delete(secure ? `__Host-${COOKIE}` : COOKIE, { path: '/', secure });
		}
	};
}

/**
 * Null only while the image is built, when none of these settings exist. A console that starts
 * without them stops with the reason, since there is no way to use it without signing in.
 */
export const identity = building ? null : start();
