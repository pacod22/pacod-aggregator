<script lang="ts">
	import { AuthCard } from '@pacod/ui';
</script>

<svelte:head>
	<title>No access · pacod-aggregator</title>
</svelte:head>

<AuthCard
	title="No access"
	subtitle="This account has no role that shows the console."
	brand="gateway"
	monogram="AG"
	wordmark="pacod-aggregator"
>
	<p class="small muted">Ask an aggregator admin for the role viewer or admin.</p>
	<form method="POST" action="/auth/logout">
		<button type="submit" class="btn">Sign out</button>
	</form>
</AuthCard>
