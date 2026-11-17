<script lang="ts">
  // Documents of the box: date, summary, tags, and buttons to look at them.
  import { app } from "../lib/app.svelte";
  import type { Doc } from "../lib/types";

  let { docs }: { docs: Doc[] } = $props();
</script>

<ul>
  {#each docs as doc (doc.hash)}
    <li>
      <span class="date">{doc.date}</span>
      <div class="text">
        <strong>{doc.summary}</strong>
        <span class="muted small">
          {#each doc.tags.split(",").filter(Boolean) as tag}<span class="tag">{tag}</span>{/each}
          {doc.original_name}
        </span>
      </div>
      <div class="buttons">
        <button class="btn" onclick={() => app.open(doc.path)}>Open</button>
        <button class="btn btn-ghost" onclick={() => app.open(doc.path, true)}>Show in Finder</button>
      </div>
    </li>
  {/each}
</ul>

<style>
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
  }

  li:hover {
    background: var(--surface-2);
  }

  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .tag {
    display: inline-block;
    margin-right: 4px;
    padding: 0 7px;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 500;
  }

  .buttons {
    display: flex;
    gap: 4px;
    opacity: 0.55;
    transition: opacity 0.12s;
  }

  li:hover .buttons,
  .buttons:focus-within {
    opacity: 1;
  }
</style>
