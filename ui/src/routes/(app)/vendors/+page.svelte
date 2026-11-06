<script lang="ts">
	import Badge from '#lib/components/Badge.svelte';
	import Outcome from '#lib/components/Outcome.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { formatDateTime } from '#lib/format.ts';
	import { may } from '#lib/nav.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const manage = $derived(may(data.principal?.permissions ?? [], 'agg.data.manage'));
</script>

<PageHeader
	title="Vendors"
	description="Whose models the aggregator follows. A vendor marked complete has every model on its pages: a model that leaves them is gone."
/>

<Outcome {form} />

<div class="table-wrap">
	<table>
		<thead>
			<tr>
				<th>Vendor</th>
				<th>Sites</th>
				<th>State</th>
				<th>Complete</th>
				<th>Last read (UTC)</th>
			</tr>
		</thead>
		<tbody>
			{#each data.vendors as vendor (vendor.id)}
				<tr>
					<td>
						<a href="/vendors/{vendor.id}">{vendor.display_name}</a>
						<div class="muted small mono">{vendor.id}</div>
					</td>
					<td class="small">{vendor.sites.join(', ') || '–'}</td>
					<td><Badge value={vendor.enabled ? 'active' : 'disabled'} /></td>
					<td>
						{#if !vendor.complete}
							–
						{:else if vendor.read_complete}
							<Badge value="ok" />
						{:else}
							<Badge value="partial" tone="warn" />
						{/if}
					</td>
					<td>{formatDateTime(vendor.checked_at)}</td>
				</tr>
			{:else}
				<tr><td colspan="5" class="muted">No vendor yet.</td></tr>
			{/each}
		</tbody>
	</table>
</div>

{#if manage}
	<details class="card">
		<summary>Add a vendor</summary>
		<form method="POST" action="?/add" class="stack">
			<label for="id">Id</label>
			<input id="id" name="id" required placeholder="acme" />
			<label for="display_name">Name</label>
			<input id="display_name" name="display_name" required placeholder="Acme AI" />
			<label for="currency">Currency</label>
			<input id="currency" name="currency" placeholder="USD" />
			<label for="sites">Where the scan starts, one address per line</label>
			<textarea id="sites" name="sites" rows="3" placeholder="https://acme.example/pricing"
			></textarea>
			<label><input type="checkbox" name="complete" /> Its pages list all its models</label>
			<div><button class="btn btn-primary" type="submit">Add vendor</button></div>
		</form>
	</details>
{/if}
