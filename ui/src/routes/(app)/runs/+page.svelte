<script lang="ts">
	import Badge from '#lib/components/Badge.svelte';
	import JsonView from '#lib/components/JsonView.svelte';
	import Outcome from '#lib/components/Outcome.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { formatDateTime } from '#lib/format.ts';
	import { may } from '#lib/nav.ts';
	import { SvelteSet } from 'svelte/reactivity';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const manage = $derived(may(data.principal?.permissions ?? [], 'agg.data.manage'));
	const outcome = (ok: boolean | null | undefined) =>
		ok === null || ok === undefined ? 'running' : ok ? 'ok' : 'failed';

	const opened = new SvelteSet<string>();
	const toggle = (id: string) => {
		if (!opened.delete(id)) opened.add(id);
	};
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
	<table class="runs">
		<thead>
			<tr>
				<th class="started">Started (UTC)</th>
				<th class="job">Job</th>
				<th class="why">Why</th>
				<th>Outcome</th>
				<th class="num covers">Covers</th>
				<th class="result">Result</th>
			</tr>
		</thead>
		<tbody>
			{#each data.runs as run (run.id)}
				<tr>
					<td class="short">{formatDateTime(run.started_at)}</td>
					<td class="short">{run.job}</td>
					<td class="short">{run.trigger}</td>
					<td>
						<Badge value={outcome(run.ok)} />
						{#if run.message}<div class="small message" title={run.message}>{run.message}</div>{/if}
					</td>
					<td class="num">{run.covers.length}</td>
					<td>
						<button
							class="btn btn-quiet"
							type="button"
							aria-expanded={opened.has(run.id)}
							aria-controls="result-{run.id}"
							onclick={() => toggle(run.id)}
						>
							{opened.has(run.id) ? 'Hide' : 'Details'}
						</button>
					</td>
				</tr>
				{#if opened.has(run.id)}
					<tr id="result-{run.id}">
						<td colspan="6">
							{#if run.message}<p class="small full-message">{run.message}</p>{/if}
							<JsonView value={run.result} what="the run's result" />
						</td>
					</tr>
				{/if}
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

<style>
	/*
	 * A fixed layout, so that a long message cannot squeeze the other columns. Those hold short
	 * values, each sized here to fit, and the console lets a cell break anywhere, so a value would
	 * otherwise be cut in the middle of a word. A run's result has a row of its own: it is far
	 * wider than any column here. A message keeps to one line, to keep the rows even and easy to
	 * scan, and the result's row shows it in full.
	 */
	.runs {
		min-width: 46rem;
		table-layout: fixed;
	}

	.started {
		width: 9.5rem;
	}

	.job,
	.why {
		width: 5.5rem;
	}

	.covers,
	.result {
		width: 6.5rem;
	}

	.runs td {
		vertical-align: top;
		overflow-wrap: anywhere;
	}

	.short {
		white-space: nowrap;
	}

	.message {
		margin-top: var(--space-1);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.full-message {
		margin-bottom: var(--space-2);
	}
</style>
