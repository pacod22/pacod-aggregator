<script lang="ts">
	import Outcome from '#lib/components/Outcome.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { may } from '#lib/nav.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const manage = $derived(may(data.principal?.permissions ?? [], 'agg.data.manage'));
	const settings = $derived(data.settings);
	const gatewayInUse = $derived(settings.mode === 'gateway');
	// What the saved mode reads with: the form may show another mode that is not saved yet.
	const modelInUse = $derived.by(() => {
		const named = gatewayInUse ? settings.gateway_model : settings.extraction_model;
		const fallback = gatewayInUse
			? settings.gateway_model_default
			: settings.extraction_model_default;
		const variable = gatewayInUse ? 'AGG_GW_MODEL' : 'AGG_LLM_MODEL';
		if (named) return `In use: ${named}, named here.`;
		if (fallback)
			return `In use: ${fallback}, from ${variable}. A name saved here takes its place.`;
		return `Pages are not read until a model is named for this mode, here or as ${variable}.`;
	});
</script>

<PageHeader
	title="Settings"
	description="When the aggregator reads, and what with. These are kept in its database."
/>

<Outcome {form} />

<form method="POST" action="?/save" class="stack-lg">
	<input type="hidden" name="version" value={settings.version} />
	<fieldset disabled={!manage} class="stack">
		<legend>Schedules</legend>
		<label>
			<input type="checkbox" name="enabled" checked={settings.enabled} />
			The check and the scan run on their schedules
		</label>
		<label for="schedule">Check of the confirmed pages, as a cron expression with five fields</label
		>
		<input id="schedule" name="schedule" class="mono" value={settings.schedule} />
		<label for="scan_schedule">Scan of the vendors' sites, in the same form</label>
		<input id="scan_schedule" name="scan_schedule" class="mono" value={settings.scan_schedule} />
		<label for="time_zone">Time zone the schedules are read in</label>
		<input id="time_zone" name="time_zone" value={settings.time_zone} />
	</fieldset>

	<fieldset disabled={!manage} class="stack">
		<legend>Extraction</legend>
		<p class="muted small">
			The one model the aggregator calls: it reads the pages. The scan and settling into the
			catalogue use none. The two endpoints and their keys are set in the environment. Here you
			choose between them, and the choice applies from the next page read, without a restart.
		</p>
		<label for="mode">How the model is reached</label>
		<!-- A select, not radios that are disabled when not set up: a disabled input is not posted,
		     so saving the page for another reason could change the mode. -->
		<select id="mode" name="mode" value={settings.mode}>
			<option value="standalone">
				Standalone: calls a provider's endpoint itself{settings.standalone_configured
					? ''
					: ' (not set up: AGG_LLM_URL is empty)'}
			</option>
			<option value="gateway">
				Gateway: calls through pacod-gw{settings.gateway_configured
					? ''
					: ' (not set up: AGG_GW_URL is empty)'}
			</option>
		</select>
		<p class="muted small">
			A gateway accounts each call, and its key can carry a spending cap. Neither applies in
			standalone mode, so set a limit at the provider.
		</p>
		<p class="muted small">{modelInUse}</p>
		<label for="extraction_model">Model in standalone mode, by the name its endpoint takes</label>
		<input
			id="extraction_model"
			name="extraction_model"
			value={settings.extraction_model ?? ''}
			placeholder={settings.extraction_model_default ?? ''}
		/>
		<label for="gateway_model">Model in gateway mode, by the name the gateway takes</label>
		<input
			id="gateway_model"
			name="gateway_model"
			value={settings.gateway_model ?? ''}
			placeholder={settings.gateway_model_default ?? ''}
		/>
		<label for="extraction_context_window">How much the model takes in at once, in tokens</label>
		<input
			id="extraction_context_window"
			name="extraction_context_window"
			type="number"
			min="1"
			value={settings.extraction_context_window}
		/>
		<label for="max_extractions_per_run">Calls of the model that one run may make</label>
		<input
			id="max_extractions_per_run"
			name="max_extractions_per_run"
			type="number"
			min="1"
			value={settings.max_extractions_per_run}
		/>
	</fieldset>

	<fieldset disabled={!manage} class="stack">
		<legend>Scan</legend>
		<label for="max_pages_per_vendor">Pages it opens per vendor</label>
		<input
			id="max_pages_per_vendor"
			name="max_pages_per_vendor"
			type="number"
			min="1"
			max="100"
			value={settings.scan?.max_pages_per_vendor}
		/>
		<label for="min_score">
			Score a page needs to be proposed: a price per million tokens counts 2, a known model 3, a
			keyword in the address or title 4
		</label>
		<input
			id="min_score"
			name="min_score"
			type="number"
			min="0"
			step="any"
			value={settings.scan?.min_score}
		/>
		<label for="keywords">Words that make a page worth opening, separated by commas</label>
		<input id="keywords" name="keywords" value={settings.scan?.keywords?.join(', ')} />
	</fieldset>

	<fieldset disabled={!manage} class="stack">
		<legend>OpenRouter</legend>
		<label for="openrouter_price_vendors">
			Vendors whose prices are taken from its list, separated by commas. For others its price is
			usually a third party's.
		</label>
		<input
			id="openrouter_price_vendors"
			name="openrouter_price_vendors"
			value={settings.openrouter_price_vendors?.join(', ')}
		/>
	</fieldset>

	{#if manage}<div><button class="btn btn-primary" type="submit">Save</button></div>{/if}
</form>
