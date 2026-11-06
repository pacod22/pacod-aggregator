<script lang="ts">
	import { evidenceUrl, facts, rates } from '#lib/catalogue.ts';
	import Badge from '#lib/components/Badge.svelte';
	import JsonView from '#lib/components/JsonView.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { formatDateTime } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	const model = $derived(data.model);
	const entry = $derived(
		model.entry as {
			properties?: unknown;
			parameters?: Record<string, unknown>;
			price?: unknown;
			provenance?: unknown;
			aliases?: string[];
		}
	);
	const properties = $derived(facts(entry.properties));
	const parameters = $derived(Object.entries(entry.parameters ?? {}));
	const source = $derived(evidenceUrl(entry.price));
</script>

<PageHeader
	title={model.api_id}
	description={model.display_name ?? undefined}
	crumbs={[{ label: 'Catalogue', href: '/catalogue' }]}
/>

<section class="card stack">
	<dl class="facts">
		<dt>Vendor</dt>
		<dd>{model.vendor}</dd>
		<dt>Status</dt>
		<dd><Badge value={model.status} /></dd>
		<dt>Kind</dt>
		<dd>{model.kind ?? '–'}</dd>
		<dt>Id at the vendor</dt>
		<dd>
			<code>{model.api_id}</code>
			{model.api_id_confirmed ? "from the vendor's own page" : 'from a list, not confirmed'}
		</dd>
		{#if entry.aliases?.length}
			<dt>Aliases</dt>
			<dd>{entry.aliases.join(', ')}</dd>
		{/if}
		<dt>Last seen (UTC)</dt>
		<dd>{formatDateTime(model.last_seen_at)}</dd>
	</dl>
</section>

<section class="card stack">
	<h2>Properties</h2>
	{#if properties.length}
		<dl class="facts">
			{#each properties as fact (fact.label)}
				<dt>{fact.label}</dt>
				<dd>{fact.value}</dd>
			{/each}
		</dl>
	{:else}
		<p class="muted">No source states any yet.</p>
	{/if}
</section>

<section class="card stack">
	<h2>Parameters</h2>
	{#if parameters.length}
		<dl class="facts">
			{#each parameters as [name, parameter] (name)}
				<dt><code>{name}</code></dt>
				<dd>
					{facts(parameter)
						.map((f) => `${f.label}: ${f.value}`)
						.join(' · ')}
				</dd>
			{/each}
		</dl>
	{:else}
		<p class="muted">No source states any yet.</p>
	{/if}
</section>

<section class="card stack">
	<h2>Price</h2>
	{#if source}
		<p class="small">
			Read from <a href={source} rel="noreferrer noopener" target="_blank">{source}</a>
		</p>
	{/if}
	<div class="table-wrap">
		<table>
			<thead>
				<tr>
					<th>In effect from (UTC)</th>
					<th>Until (UTC)</th>
					<th>Rates per 1M tokens</th>
					<th>Last confirmed (UTC)</th>
				</tr>
			</thead>
			<tbody>
				{#each model.prices as row (row.id)}
					<tr>
						<td>{formatDateTime(row.effective_from)}</td>
						<td>{formatDateTime(row.superseded_at) || 'now'}</td>
						<td>
							{rates(row.price)
								.map((f) => `${f.label} ${f.value}`)
								.join(' · ')}
						</td>
						<td>{formatDateTime(row.checked_at)}</td>
					</tr>
				{:else}
					<tr><td colspan="4" class="muted">No price yet.</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
</section>

{#if model.changes.length}
	<section class="card stack">
		<h2>Changes</h2>
		<div class="table-wrap">
			<table>
				<thead>
					<tr><th>When (UTC)</th><th>What</th><th>From</th><th>To</th></tr>
				</thead>
				<tbody>
					{#each model.changes as change (change.id)}
						<tr>
							<td>{formatDateTime(change.at)}</td>
							<td>{change.field.replaceAll('_', ' ')}</td>
							<td class="mono small">{JSON.stringify(change.old) ?? '–'}</td>
							<td class="mono small">{JSON.stringify(change.new) ?? '–'}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	</section>
{/if}

<details class="card">
	<summary>The entry as the catalogue publishes it</summary>
	<JsonView value={model.entry} what="the entry" />
</details>
