<script lang="ts">
  // Review the AI's proposal for each file before it goes into the box.
  import { app } from "../lib/app.svelte";
  import * as api from "../lib/api";

  let validDate = $state(true);

  $effect(() => {
    const date = app.current?.entry.date;
    if (date === undefined) return;
    api.isIsoDate(date).then((ok) => (validDate = ok || date.trim() === "unknown"));
  });

  function submit(e: SubmitEvent) {
    e.preventDefault();
    app.saveCurrent();
  }
</script>

<div class="layout">
  {#if app.queue.length > 0}
    <p class="muted small waiting">Waiting: {app.queue.map(api.basename).join(", ")}</p>
  {/if}

  {#if app.current}
    {@const current = app.current}
    <form class="card review" onsubmit={submit}>
      <div class="head">
        <div>
          <h2>{current.proposal.name}</h2>
          <p class="muted small">Check the proposal, change what is wrong, then save.</p>
        </div>
        <button class="btn" type="button" onclick={() => app.open(current.proposal.file)}>Open PDF</button>
      </div>
      <label class="field date-field">
        <span>Date</span>
        <input type="text" bind:value={current.entry.date} placeholder="YYYY-MM-DD" />
        {#if !validDate}<span class="hint">YYYY-MM-DD or unknown</span>{/if}
      </label>
      <label class="field">
        <span>Tags</span>
        <input type="text" bind:value={current.entry.tags} placeholder="comma, separated" />
      </label>
      <label class="field">
        <span>Summary</span>
        <textarea rows="3" bind:value={current.entry.summary}></textarea>
      </label>
      <div class="actions">
        <button class="btn btn-primary" type="submit">Save to box</button>
        <button class="btn" type="button" onclick={() => app.skipCurrent()}>Skip</button>
      </div>
    </form>
  {:else if !app.busy}
    <section class="card empty">
      <p>Nothing to review.</p>
      <p class="muted small">Open PDFs in Files and choose "Add to box".</p>
    </section>
  {/if}
</div>

<style>
  .layout {
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow: auto;
    min-height: 0;
  }

  .waiting {
    padding: 0 2px;
  }

  .review {
    display: flex;
    flex-direction: column;
    gap: 14px;
    max-width: 720px;
    padding: 16px;
  }

  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  .head h2 {
    overflow-wrap: anywhere;
  }

  .date-field input {
    max-width: 180px;
  }

  textarea {
    min-height: 80px;
  }

  .actions {
    display: flex;
    gap: 8px;
  }

  .empty {
    padding: 40px 16px;
    text-align: center;
  }

  .empty p + p {
    margin-top: 4px;
  }
</style>
