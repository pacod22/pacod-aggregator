import { describe, expect, it } from 'vitest';
import { evidenceUrl, facts, rate, rates } from './catalogue';

describe('facts', () => {
	it('names what the entry says and leaves out what it does not', () => {
		expect(
			facts({ context_window: 128000, vision: true, tools: false, reasoning: null, future: 'x' })
		).toEqual([
			{ label: 'Context window', value: '128,000' },
			{ label: 'Vision', value: 'yes' },
			{ label: 'Tools', value: 'no' },
			{ label: 'Future', value: 'x' }
		]);
	});

	it('shows lists and nested values', () => {
		expect(
			facts({ formats: ['pdf', 'png'], modalities: { input: ['text', 'image'], output: ['text'] } })
		).toEqual([
			{ label: 'Formats', value: 'pdf, png' },
			{ label: 'Modalities', value: 'input: text, image; output: text' }
		]);
	});

	it('has nothing to say of nothing', () => {
		expect(facts(null)).toEqual([]);
		expect(facts(undefined)).toEqual([]);
	});
});

describe('rates', () => {
	it('keeps the digits a price was stated with', () => {
		expect(rate(2.2, 'USD')).toBe('2.20 USD');
		expect(rate(0.000125, 'USD')).toBe('0.000125 USD');
		expect(rate(0)).toBe('0.00');
		expect(rate(null, 'USD')).toBe('');
	});

	it('lists the rates a price has', () => {
		expect(
			rates({ currency: 'USD', rates: { input: 2.2, cache_read: 0.22, output: null } })
		).toEqual([
			{ label: 'Input', value: '2.20 USD' },
			{ label: 'Cached input', value: '0.22 USD' }
		]);
		expect(rates(null)).toEqual([]);
	});
});

describe('evidenceUrl', () => {
	it('takes an address that a link may open', () => {
		expect(evidenceUrl({ evidence: { url: 'https://x.example/pricing' } })).toBe(
			'https://x.example/pricing'
		);
	});

	it('refuses anything else, since the address comes from a page', () => {
		expect(evidenceUrl({ evidence: { url: 'javascript:alert(1)' } })).toBeNull();
		expect(evidenceUrl({})).toBeNull();
		expect(evidenceUrl(null)).toBeNull();
	});
});
