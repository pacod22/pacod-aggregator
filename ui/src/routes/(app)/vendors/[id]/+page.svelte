<script lang="ts">
	import Outcome from '#lib/components/Outcome.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { may } from '#lib/nav.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const manage = $derived(may(data.principal?.permissions ?? [], 'agg.data.manage'));
	const vendor = $derived(data.vendor);
</script>

<PageHeader
	title={vendor.display_name}
	description={vendor.id}
	crumbs={[{ label: 'Vendors', href: '/vendors' }]}
/>

<Outcome {form} />

<p>
	<a href="/pages?vendor_id={vendor.id}">{data.pages.length} pages</a>
	·
	<a href="/catalogue?vendor={vendor.id}">its models</a>
</p>

<section class="card">
	<form method="POST" action="?/save" class="stack">
		<fieldset disabled={!manage} class="plain stack">
			<label for="display_name">Name</label>
			<input id="display_name" name="display_name" required value={vendor.display_name} />
			<label for="currency">Currency</label>
			<input id="currency" name="currency" value={vendor.currency} />
			<label for="sites">Where the scan starts, one address per line</label>
			<textarea id="sites" name="sites" rows="4" value={vendor.sites.join('\n')}></textarea>
			<label><input type="checkbox" name="enabled" checked={vendor.enabled} /> Followed</label>
			<label>
				<input type="checkbox" name="complete" checked={vendor.complete} />
				Its pages list all its models
			</label>
			{#if manage}<div><button class="btn btn-primary" type="submit">Save</button></div>{/if}
		</fieldset>
	</form>
</section>

{#if manage}
	<details class="card">
		<summary>Remove this vendor</summary>
		<form method="POST" action="?/remove" class="stack">
			<p>
				Its pages and models leave the catalogue. A vendor the installation started with does not
				come back by itself.
			</p>
			<div><button class="btn btn-danger" type="submit">Remove vendor</button></div>
		</form>
	</details>
{/if}

<style>
	.plain {
		padding: 0;
		border: 0;
	}
</style>
