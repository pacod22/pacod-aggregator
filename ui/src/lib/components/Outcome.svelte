<script lang="ts">
	import Notice from './Notice.svelte';

	/** What came of a form action. */
	interface Props {
		form?: { done?: string; message?: string } | null;
	}

	let { form }: Props = $props();
</script>

{#if form?.message}<Notice tone="error">{form.message}</Notice>{/if}
{#if form?.done}<Notice tone="success">{form.done}</Notice>{/if}
