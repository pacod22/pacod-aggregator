import { describe, expect, it } from 'vitest';
import type { components } from '#lib/api/schema.d.ts';
import { gatewayChoices } from './gateway';

type Offer = components['schemas']['Offer'];

const offer = (over: Partial<Offer>): Offer => ({
	id: 'openai/gpt-x',
	kind: 'model',
	structured_output: true,
	deprecated: false,
	...over
});

const offers = [
	offer({ id: '@reader', kind: 'template', uses: 'openai/gpt-x' }),
	offer({ id: 'openai/gpt-x' }),
	offer({ id: 'openai/embed', structured_output: false, deprecated: true }),
	offer({ id: 'other/unknown', structured_output: null })
];

describe('gatewayChoices', () => {
	it('keeps templates and models apart, and says what a template uses now', () => {
		const c = gatewayChoices(offers, null, true);
		expect(c.templates).toEqual([{ value: '@reader', label: '@reader (now openai/gpt-x)' }]);
		expect(c.models.map((m) => m.value)).toEqual(['openai/gpt-x', 'openai/embed', 'other/unknown']);
		expect(c.missing).toBeNull();
	});

	it('marks a model that cannot read pages instead of leaving it out', () => {
		const embed = gatewayChoices(offers, null, true).models.find((m) => m.value === 'openai/embed');
		expect(embed?.label).toBe(
			'openai/embed (no structured output, so it cannot read pages; deprecated)'
		);
		// Unknown is not the same as unable.
		const unknown = gatewayChoices(offers, null, true).models.find(
			(m) => m.value === 'other/unknown'
		);
		expect(unknown?.label).toBe('other/unknown');
	});

	it('keeps a saved name the gateway no longer offers, so saving the page does not change it', () => {
		expect(gatewayChoices(offers, 'openai/gone', true).missing).toEqual({
			value: 'openai/gone',
			label: 'openai/gone (not offered by the gateway now)'
		});
		expect(gatewayChoices(offers, '@reader', true).missing).toBeNull();
	});

	it('does not call a saved name unoffered when the list could not be read', () => {
		expect(gatewayChoices([], '@reader', false).missing).toEqual({
			value: '@reader',
			label: '@reader (saved)'
		});
	});
});
