<!-- gw's Pricing pages (commit 50fe4b7, ui/src/routes/(app)/platform/price-tracker/pages), for vendors. -->
<script lang="ts">
	import Badge from '#lib/components/Badge.svelte';
	import Outcome from '#lib/components/Outcome.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { formatDateTime } from '#lib/format.ts';
	import { may } from '#lib/nav.ts';
	import { pageGroups, pageNote } from '#lib/pages.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const manage = $derived(may(data.principal?.permissions ?? [], 'agg.data.manage'));
	const groups = $derived(pageGroups(data.pages));
	const waiting = $derived(data.pages.filter((p) => p.status === 'proposed').length);
</script>

<PageHeader
	title="Pages"
	description="The pages the aggregator reads for each vendor: pricing pages, model pages and machine-readable lists. The weekly scan proposes the pages it finds; a page is read once it is confirmed."
/>

<Outcome {form} />

<form method="GET" class="row filters">
	<label>
		Status
		<select name="status" value={data.status ?? ''}>
			<option value="">Any status</option>
			<option value="proposed">proposed</option>
			<option value="active">active</option>
			<option value="dismissed">dismissed</option>
		</select>
	</label>
	<label>
		Vendor
		<select name="vendor_id" value={data.vendor_id ?? ''}>
			<option value="">All vendors</option>
			{#each data.vendors as vendor (vendor.id)}
				<option value={vendor.id}>{vendor.display_name}</option>
			{/each}
		</select>
	</label>
	<button class="btn" type="submit">Show</button>
</form>

{#if waiting > 0}
	<p class="muted">
		{waiting} page{waiting === 1 ? ' waits' : 's wait'} to be confirmed. Confirm the pages that list a
		vendor's models and prices; dismiss the others, and the scan does not propose them again.
	</p>
{/if}

{#each groups as group (group.vendor_id)}
	<section class="stack">
		<h2 class="vendor">{group.vendor}</h2>
		<div class="table-wrap">
			<table>
				<thead>
					<tr>
						<th>Page</th>
						<th>Status</th>
						<th>Found by</th>
						<th>Last checked (UTC)</th>
						<th>What the last check found</th>
						{#if manage}<th><span class="visually-hidden">Actions</span></th>{/if}
					</tr>
				</thead>
				<tbody>
					{#each group.pages as page (page.id)}
						<tr>
							<td>
								<a href={page.url} rel="noreferrer noopener" target="_blank">{page.url}</a>
								{#if page.title}<div class="muted small">{page.title}</div>{/if}
							</td>
							<td><Badge value={page.status} /></td>
							<td>{page.found_by}</td>
							<td>{formatDateTime(page.state?.last_checked_at) || 'Not yet'}</td>
							<td class="small">{pageNote(page)}</td>
							{#if manage}
								<td class="actions">
									{#if page.status !== 'active'}
										<form method="POST" action="?/confirm">
											<input type="hidden" name="id" value={page.id} />
											<input type="hidden" name="url" value={page.url} />
											<button type="submit" class="btn btn-quiet"
												>Confirm<span class="visually-hidden"> {page.url}</span></button
											>
										</form>
									{:else}
										<form method="POST" action="?/check">
											<input type="hidden" name="id" value={page.id} />
											<input type="hidden" name="url" value={page.url} />
											<button
												type="submit"
												class="btn btn-quiet"
												disabled={page.state?.check_requested}
												>Check now<span class="visually-hidden"> {page.url}</span></button
											>
										</form>
									{/if}
									{#if page.status !== 'dismissed'}
										<form method="POST" action="?/dismiss">
											<input type="hidden" name="id" value={page.id} />
											<input type="hidden" name="url" value={page.url} />
											<button type="submit" class="btn btn-quiet"
												>Dismiss<span class="visually-hidden"> {page.url}</span></button
											>
										</form>
									{/if}
									<details>
										<summary class="btn btn-quiet danger">Remove</summary>
										<form method="POST" action="?/remove" class="stack">
											<input type="hidden" name="id" value={page.id} />
											<input type="hidden" name="url" value={page.url} />
											<p class="small">
												It is no longer read, and what was read from it stays. The scan may propose
												it again; dismiss it instead to keep it out.
											</p>
											<button type="submit" class="btn btn-danger"
												>Remove page<span class="visually-hidden"> {page.url}</span></button
											>
										</form>
									</details>
								</td>
							{/if}
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	</section>
{:else}
	<div class="card muted">No page here. The weekly scan proposes the pages it finds.</div>
{/each}

{#if manage}
	<details class="card">
		<summary>Add a page</summary>
		<form method="POST" action="?/add" class="stack">
			<p class="muted small">
				The page is read at the next check. A page is rendered in the browser and read by the
				extraction model; a list is read as it is.
			</p>
			<label for="vendor_id">Vendor</label>
			<select id="vendor_id" name="vendor_id" required>
				{#each data.vendors as vendor (vendor.id)}
					<option value={vendor.id}>{vendor.display_name}</option>
				{/each}
			</select>
			<label for="url">Address</label>
			<input id="url" name="url" type="url" required />
			<label for="kind">Kind</label>
			<select id="kind" name="kind">
				<option value="page">page: rendered and read by the model</option>
				<option value="feed">feed: a list, read as it is</option>
			</select>
			<label for="format">Format of a feed</label>
			<select id="format" name="format">
				<option value="">not a feed</option>
				<option value="pacod">pacod</option>
				<option value="litellm">litellm</option>
				<option value="openrouter">openrouter</option>
			</select>
			<label for="priority">
				Priority: where two pages disagree about a model, the lower number wins
			</label>
			<input id="priority" name="priority" type="number" value="0" />
			<label for="notes">Notes</label>
			<input id="notes" name="notes" />
			<div><button class="btn btn-primary" type="submit">Add page</button></div>
		</form>
	</details>
{/if}

<style>
	.vendor {
		font-size: var(--text-lg);
	}

	.filters {
		align-items: flex-end;
		flex-wrap: wrap;
		gap: var(--space-3);
	}

	.filters select {
		width: auto;
	}

	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-1);
	}
</style>
