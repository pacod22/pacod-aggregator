<script lang="ts">
	import Badge from '#lib/components/Badge.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { formatDateTime } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	const status = $derived(data.status);
	const jobs = $derived(
		status
			? [
					{ name: 'Check', what: 'Reads the confirmed pages', job: status.check },
					{ name: 'Scan', what: "Looks for pages on the vendors' sites", job: status.scan }
				]
			: []
	);
</script>

<PageHeader title="Overview" description="The scraper, its next runs, and what needs attention." />

{#if status}
	{#each status.problems as problem (problem)}
		<Notice tone="warning">{problem}</Notice>
	{/each}
	{#if status.proposed_pages > 0}
		<Notice>
			<p>
				<a href="/pages?status=proposed">{status.proposed_pages} pages</a> wait for a decision.
			</p>
		</Notice>
	{/if}

	<section class="card stack">
		<h2>Scraper</h2>
		<dl class="facts">
			<dt>State</dt>
			<dd><Badge value={status.scraper.present ? 'present' : 'absent'} /></dd>
			<dt>Last seen (UTC)</dt>
			<dd>{formatDateTime(status.scraper.last_seen_at) || 'never'}</dd>
			<dt>Browser</dt>
			<dd>
				{#if status.scraper.browser}
					<Badge value={status.scraper.browser.ok ? 'ok' : 'failed'} />
					{status.scraper.browser.product || status.scraper.browser.message}
				{:else}
					not reported yet
				{/if}
			</dd>
			<dt>Extraction</dt>
			<dd>
				<Badge value={status.extraction ? 'ok' : 'not_configured'} />
				{status.extraction ? 'Pages are read by the model' : 'Pages are fetched but not read'}
			</dd>
		</dl>
	</section>

	{#each jobs as { name, what, job } (name)}
		<section class="card stack">
			<h2>{name}</h2>
			<p class="muted">{what}.</p>
			<dl class="facts">
				<dt>Schedule</dt>
				<dd><code>{job.schedule}</code></dd>
				<dt>Next run (UTC)</dt>
				<dd>
					{#if job.running}running now{:else}{formatDateTime(job.next_run_at) || 'none'}{/if}
				</dd>
				<dt>Last run (UTC)</dt>
				<dd>
					{#if job.last_run}
						<Badge value={job.last_run.ok === false ? 'failed' : 'ok'} />
						{formatDateTime(job.last_run.started_at)}
						{job.last_run.message}
					{:else}
						none yet
					{/if}
				</dd>
			</dl>
		</section>
	{/each}
{/if}
