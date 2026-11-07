/** The choices of the gateway's model on the settings page, from what the gateway offers. */
import type { components } from '#lib/api/schema.d.ts';

type Offer = components['schemas']['Offer'];

export interface Choice {
	value: string;
	label: string;
}

export interface Choices {
	templates: Choice[];
	models: Choice[];
	/** The saved name, where the gateway does not offer it now. */
	missing: Choice | null;
}

function label(offer: Offer): string {
	const notes = [
		offer.kind === 'template' && offer.uses ? `now ${offer.uses}` : '',
		offer.structured_output === false ? 'no structured output, so it cannot read pages' : '',
		offer.deprecated ? 'deprecated' : ''
	].filter(Boolean);
	return notes.length > 0 ? `${offer.id} (${notes.join('; ')})` : offer.id;
}

/**
 * Templates and models apart, as the gateway lists them. Every offer stays a choice: one that
 * cannot read pages is marked, not left out, because the list is what the gateway offers. The saved
 * name stays a choice too, so that saving the page as it stands never changes it. It is called not
 * offered only when the list was read (`listed`): without a list, nobody knows.
 */
export function gatewayChoices(
	offers: Offer[],
	saved: string | null | undefined,
	listed: boolean
): Choices {
	const of = (kind: Offer['kind']) =>
		offers.filter((o) => o.kind === kind).map((o) => ({ value: o.id, label: label(o) }));
	const offered = saved && offers.some((o) => o.id === saved);
	const note = listed ? 'not offered by the gateway now' : 'saved';
	return {
		templates: of('template'),
		models: of('model'),
		missing: saved && !offered ? { value: saved, label: `${saved} (${note})` } : null
	};
}
