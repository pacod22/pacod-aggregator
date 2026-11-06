// The console's settings, from pacod-gw's src/env.ts (commit 50fe4b7) as pacod-broker's console cut
// it down to one product.
import { defineEnvVars } from '@sveltejs/kit/env';

/** An optional http(s) URL setting, without a trailing slash. */
function optionalUrl(name: string, example: string) {
	return (value: string | undefined) => {
		const text = (value ?? '').trim();
		if (text === '') return undefined;
		let url: URL;
		try {
			url = new URL(text);
		} catch {
			throw new Error(`${name} must be a URL such as ${example}, got "${text}"`);
		}
		if (url.protocol !== 'http:' && url.protocol !== 'https:') {
			throw new Error(`${name} must start with http:// or https://`);
		}
		return text.replace(/\/+$/, '');
	};
}

/** An optional secret: trimmed, empty when unset. */
const secret = (value: string | undefined) => (value ?? '').trim() || undefined;

export const variables = defineEnvVars({
	AGG_CONSOLE_API_URL: {
		description:
			"The aggregator's admin API, as the console's server reaches it, such as http://pacod-aggregator-admin:8151. Server-only: the browser never talks to it.",
		schema: (value) => (value ?? '').trim().replace(/\/+$/, '') || 'http://127.0.0.1:18151'
	},
	AGG_ID_ISSUER: {
		description: "pacod-id's public issuer, such as https://id.example.com.",
		schema: optionalUrl('AGG_ID_ISSUER', 'https://id.example.com')
	},
	AGG_ID_INTERNAL_URL: {
		description:
			"pacod-id's internal listener as the console's server reaches it, such as http://pacod-id-internal:8071. Default: the issuer.",
		schema: optionalUrl('AGG_ID_INTERNAL_URL', 'http://pacod-id-internal:8071')
	},
	AGG_UI_CLIENT_ID: {
		description: "The console's client at pacod-id. Default pacod-agg-ui.",
		schema: (value) => (value ?? '').trim() || 'pacod-agg-ui'
	},
	AGG_UI_CLIENT_SECRET: {
		description: "The console's client secret at pacod-id.",
		schema: secret
	},
	AGG_UI_SESSION_SECRET: {
		description:
			"At least 32 characters: seals the session cookie that holds pacod-id's tokens. Changing it signs everyone out.",
		schema: secret
	},
	AGG_CONSOLE_URL: {
		description:
			"The console's address as people reach it, such as https://agg.example.com: where pacod-id sends them back.",
		schema: optionalUrl('AGG_CONSOLE_URL', 'https://agg.example.com')
	}
});
