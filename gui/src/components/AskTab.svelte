<script lang="ts">
  // Answers from the text inside the documents, with the source pages and checked quotes.
  import { app } from "../lib/app.svelte";
  import { REVEAL_LABEL } from "../lib/api";

  function submit(e: SubmitEvent) {
    e.preventDefault();
    app.startAsk();
  }
</script>

<div class="layout">
  <form class="card ask" onsubmit={submit}>
    <input
      type="search"
      placeholder="Ask about the content, e.g. How much is the rent?"
      bind:value={app.askQuestion}
      disabled={!!app.busy}
      aria-label="Question"
    />
    <button class="btn btn-primary" type="submit" disabled={!!app.busy || !app.askQuestion.trim()}>Ask</button>
  </form>

  <section class="card answer">
    {#if !app.asked}
      <div class="empty">
        <p>Ask a question about your documents.</p>
        <p class="muted small">
          Answers come from the text inside the documents. Every answer shows its source pages with a quote that is
          checked; if the check fails, the answer is “Not found”.
        </p>
      </div>
    {:else}
      {@const asked = app.asked}
      <p class="muted small">“{asked.question}”</p>
      {#if asked.outcome.kind === "answered"}
        <p class="reply">{asked.outcome.answer}</p>
        <h3 class="section-title">Sources</h3>
        {#each asked.outcome.sources as source, i (i)}
          <div class="source">
            <div class="text">
              <strong>{source.name}, page {source.page}</strong>
              <blockquote>{source.quote}</blockquote>
            </div>
            <div class="buttons">
              <button class="btn" onclick={() => app.open(source.path)}>Open</button>
              <button class="btn btn-ghost" onclick={() => app.open(source.path, true)}>{REVEAL_LABEL}</button>
            </div>
          </div>
        {/each}
      {:else}
        <p class="reply">Not found in your documents.</p>
        <p class="muted">({asked.outcome.why})</p>
        {#if asked.outcome.claimed}
          <p class="muted small">The model said, unchecked: {asked.outcome.claimed}</p>
        {/if}
      {/if}
    {/if}
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

  .answer {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 16px;
    overflow: auto;
  }

  .reply {
    font-size: 17px;
    font-weight: 600;
    white-space: pre-wrap;
  }

  .section-title {
    margin-top: 8px;
  }

  .source {
    display: flex;
    align-items: flex-start;
    gap: 14px;
    padding-top: 10px;
    border-top: 1px solid var(--border);
  }

  .text {
    flex: 1;
    min-width: 0;
  }

  blockquote {
    margin: 6px 0 0;
    padding: 4px 0 4px 10px;
    border-left: 3px solid var(--accent);
    color: var(--muted);
  }

  .buttons {
    display: flex;
    gap: 4px;
  }

  .empty {
    margin: auto;
    max-width: 60ch;
    padding: 40px 16px;
    text-align: center;
  }

  .empty p + p {
    margin-top: 4px;
  }
</style>
