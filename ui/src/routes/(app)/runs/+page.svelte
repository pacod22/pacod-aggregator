<script lang="ts">
	import Badge from '#lib/components/Badge.svelte';
	import JsonView from '#lib/components/JsonView.svelte';
	import Outcome from '#lib/components/Outcome.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { formatDateTime } from '#lib/format.ts';
	import { may } from '#lib/nav.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const manage = $derived(may(data.principal?.permissions ?? [], 'agg.data.manage'));
	const outcome = (ok: boolean | null | undefined) =>
		ok === null || ok === undefined ? 'running' : ok ? 'ok' : 'failed';
</script>

<PageHeader
	title="Runs"
	description="What the check and the scan did, and what they changed in the catalogue."
>
	{#snippet actions()}
		{#if manage}
			<form method="POST" action="?/check">
				<button class="btn" type="submit">Check now</button>
			</form>
			<form method="POST" action="?/scan">
				<button class="btn" type="submit">Scan now</button>
			</form>
		{/if}
	{/snippet}
</PageHeader>

<Outcome {form} />

<div class="table-wrap">
	<table>
		<thead>
			<tr>
				<th>Started (UTC)</th>
				<th>Job</th>
				<th>Why</th>
				<th>Outcome</th>
				<th class="num">Covers</th>
				<th>Result</th>
			</tr>
		</thead>
		<tbody>
			{#each data.runs as run (run.id)}
				<tr>
					<td>{formatDateTime(run.started_at)}</td>
					<td>{run.job}</td>
					<td>{run.trigger}</td>
					<td>
						<Badge value={outcome(run.ok)} />
						{#if run.message}<div class="small">{run.message}</div>{/if}
					</td>
					<td class="num">{run.covers.length}</td>
					<td>
						<details>
							<summary>Details</summary>
							<JsonView value={run.result} what="the run's result" />
						</details>
					</td>
				</tr>
			{:else}
				<tr><td colspan="6" class="muted">No run yet.</td></tr>
			{/each}
		</tbody>
	</table>
</div>

<section class="stack">
	<h2>Latest changes</h2>
	<div class="table-wrap">
		<table>
			<thead>
				<tr><th>When (UTC)</th><th>Model</th><th>What</th><th>From</th><th>To</th></tr>
			</thead>
			<tbody>
				{#each data.changes as change (change.id)}
					<tr>
						<td>{formatDateTime(change.at)}</td>
						<td><a href="/catalogue/{change.model_id}">{change.model_id}</a></td>
						<td>{change.field.replaceAll('_', ' ')}</td>
						<td class="mono small">{JSON.stringify(change.old) ?? '–'}</td>
						<td class="mono small">{JSON.stringify(change.new) ?? '–'}</td>
					</tr>
				{:else}
					<tr><td colspan="5" class="muted">Nothing has changed yet.</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
</section>
