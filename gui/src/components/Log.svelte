<script lang="ts">
  // Progress lines in a collapsible list; follows the newest line while a job runs.
  interface Props {
    lines: string[];
    follow?: boolean;
    open?: boolean;
  }
  let { lines, follow = false, open = false }: Props = $props();

  let box = $state<HTMLElement | null>(null);

  $effect(() => {
    lines.length;
    if (follow && box) box.scrollTop = box.scrollHeight;
  });
</script>

<details {open}>
  <summary class="small muted">Log ({lines.length} lines)</summary>
  <div class="log" bind:this={box}>
    {#each lines as line, i (i)}<div>{line}</div>{/each}
  </div>
</details>

<style>
  summary {
    cursor: pointer;
    user-select: none;
  }

  .log {
    max-height: 260px;
    margin-top: 8px;
    padding: 8px 10px;
    overflow: auto;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
    white-space: pre-wrap;
  }
</style>
