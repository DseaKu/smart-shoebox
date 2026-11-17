<script lang="ts">
  // All documents, newest first, with a filter; or the matches of a question to the AI.
  import { app } from "../lib/app.svelte";
  import DocumentList from "./DocumentList.svelte";

  let filter = $state("");

  let shown = $derived.by(() => {
    if (app.results) return app.results.rows;
    const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
    return app.docs.filter((d) => {
      const hay = `${d.date} ${d.summary} ${d.tags} ${d.original_name}`.toLowerCase();
      return words.every((w) => hay.includes(w));
    });
  });

  function submit(e: SubmitEvent) {
    e.preventDefault();
    app.startSearch();
  }
</script>

<div class="layout">
  <form class="card ask" onsubmit={submit}>
    <input
      type="search"
      placeholder="Ask in plain language, e.g. Where is my rental contract?"
      bind:value={app.question}
      disabled={!!app.busy}
      aria-label="Question"
    />
    <button class="btn btn-primary" type="submit" disabled={!!app.busy || !app.question.trim()}>Search</button>
  </form>

  <section class="card list">
    <div class="toolbar">
      <div class="title">
        {#if app.results}
          <h2>Matches for “{app.results.question}”</h2>
          <span class="muted small">{app.results.rows.length}</span>
        {:else}
          <h2>All documents</h2>
          <span class="muted small">{app.docs.length}</span>
        {/if}
      </div>
      {#if app.results}
        <button class="btn" onclick={() => app.showAll()}>Show all</button>
      {:else if app.docs.length > 0}
        <input type="search" class="filter" placeholder="Filter" bind:value={filter} aria-label="Filter the documents" />
      {/if}
    </div>
    <div class="scroll">
      {#if shown.length === 0}
        <div class="empty">
          {#if app.results}
            <p>Nothing found.</p>
          {:else if app.docs.length > 0}
            <p class="muted">No document matches the filter.</p>
          {:else}
            <p>The box is empty.</p>
            <p class="muted small">Use the Wizard, or open PDFs in Files and choose "Add to box".</p>
          {/if}
        </div>
      {:else}
        <DocumentList docs={shown} />
      {/if}
    </div>
  </section>
</div>

<style>
  .layout {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .ask {
    display: flex;
    gap: 8px;
    padding: 12px 14px;
  }

  .list {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border);
  }

  .title {
    display: flex;
    align-items: baseline;
    gap: 10px;
    min-width: 0;
  }

  .filter {
    max-width: 240px;
  }

  .scroll {
    flex: 1;
    overflow: auto;
  }

  .empty {
    padding: 40px 16px;
    text-align: center;
  }

  .empty p + p {
    margin-top: 4px;
  }
</style>
