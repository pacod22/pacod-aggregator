<script lang="ts">
	import { rate } from '#lib/catalogue.ts';
	import Badge from '#lib/components/Badge.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { formatDateTime, formatInteger } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const next = $derived.by(() => {
		if (!data.models.next_cursor) return null;
		const query = new URLSearchParams({ cursor: data.models.next_cursor });
		if (data.vendor) query.set('vendor', data.vendor);
		if (data.status) query.set('status', data.status);
		return `?${query}`;
	});
</script>

<PageHeader
	title="Catalogue"
	description="What the aggregator publishes: the models of each vendor, with their properties and prices."
/>

<form method="GET" class="row filters">
	<label>
		Vendor
		<select name="vendor" value={data.vendor ?? ''}>
			<option value="">All vendors</option>
			{#each data.vendors as vendor (vendor.id)}
				<option value={vendor.id}>{vendor.display_name}</option>
			{/each}
		</select>
	</label>
	<label>
		Status
		<select name="status" value={data.status ?? ''}>
			<option value="">Any status</option>
			<option value="active">active</option>
			<option value="deprecated">deprecated</option>
			<option value="retired">retired</option>
		</select>
	</label>
	<button class="btn" type="submit">Show</button>
</form>

<div class="table-wrap">
	<table>
		<thead>
			<tr>
				<th>Model</th>
				<th>Vendor</th>
				<th>Status</th>
				<th class="num">Context</th>
				<th class="num">Input per 1M</th>
				<th class="num">Output per 1M</th>
				<th>Checked (UTC)</th>
			</tr>
		</thead>
		<tbody>
			{#each data.models.items as model (model.id)}
				<tr>
					<td>
						<a href="/catalogue/{model.id}">{model.api_id}</a>
						{#if model.display_name}<div class="muted small">{model.display_name}</div>{/if}
					</td>
					<td>{model.vendor}</td>
					<td><Badge value={model.status} /></td>
					<td class="num">{formatInteger(model.context_window)}</td>
					<td class="num">{rate(model.input, model.currency)}</td>
					<td class="num">{rate(model.output, model.currency)}</td>
					<td>{formatDateTime(model.checked_at)}</td>
				</tr>
			{:else}
				<tr><td colspan="7" class="muted">No model yet.</td></tr>
			{/each}
		</tbody>
	</table>
</div>

{#if next}<p><a href={next}>Next models</a></p>{/if}

<style>
	.filters {
		align-items: flex-end;
		flex-wrap: wrap;
		gap: var(--space-3);
	}

	.filters select {
		width: auto;
	}
</style>
