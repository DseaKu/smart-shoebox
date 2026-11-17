<script lang="ts">
  // Open PDFs (dialog or drop), select some, and run an action on them:
  // add to the box, name, merge, sort, check or look for a counterpart.
  import { open } from "@tauri-apps/plugin-dialog";
  import { app } from "../lib/app.svelte";
  import * as api from "../lib/api";
  import Pane from "./Pane.svelte";

  let idle = $derived(!app.busy);
  let selected = $derived(app.selected);
  let n = $derived(selected.length);
  let allSelected = $derived(app.files.length > 0 && n === app.files.length);

  async function choose() {
    const paths = await open({ multiple: true, filters: [{ name: "PDF", extensions: ["pdf", "PDF"] }] });
    if (paths) app.openFiles(paths);
  }

  function toggleAll() {
    const value = !allSelected;
    for (const f of app.files) f.selected = value;
  }

  const actions = $derived([
    { label: "Add to box", hint: "AI proposes date, tags, summary", ok: n >= 1, run: () => app.queueForBox(selected) },
    { label: "Name", hint: "Rename to a readable name", ok: n >= 1, run: () => app.name(selected) },
    { label: "Merge", hint: "Append into one PDF, in list order", ok: n >= 2, run: () => app.merge(selected) },
    { label: "Sort", hint: "Put the pages into reading order", ok: n === 1, run: () => app.sort(selected[0]) },
    { label: "Check", hint: "Pages complete? Name readable? In the box?", ok: n >= 1, run: () => app.check(selected) },
    {
      label: "Counterpart",
      hint: "Look for the missing pages in the other PDFs of its folder",
      ok: n === 1,
      run: () => app.counterpart(selected[0]),
    },
  ]);
</script>

<div class="layout">
  <section class="card files">
    <div class="toolbar">
      <div class="title">
        <h2>Files</h2>
        <span class="muted small">Drop PDFs on the window. Removing from the list keeps the files.</span>
      </div>
      <div class="row">
        <button class="btn btn-primary" onclick={choose}>Open PDFs…</button>
        <button class="btn" onclick={() => (app.files = app.files.filter((f) => !f.selected))} disabled={n === 0}>
          Remove from list
        </button>
      </div>
    </div>

    {#if app.files.length === 0}
      <div class="empty">
        <p>No files open.</p>
        <p class="muted small">Click "Open PDFs…" or drop PDFs on the window.</p>
      </div>
    {:else}
      <div class="list">
        <label class="check all small muted">
          <input type="checkbox" checked={allSelected} onchange={toggleAll} />
          {n} of {app.files.length} selected
        </label>
        {#each app.files as file, i (file.path)}
          <div class="file" class:selected={file.selected}>
            <label class="check grow">
              <input type="checkbox" bind:checked={file.selected} />
              <span class="name">
                <strong>{api.basename(file.path)}</strong>
                <span class="muted small">{api.dirname(file.path)}</span>
              </span>
            </label>
            <button
              class="btn btn-ghost btn-icon"
              title="Move up (order for merge)"
              disabled={i === 0}
              onclick={() => app.move(i - 1, i)}>↑</button
            >
            <button
              class="btn btn-ghost btn-icon"
              title="Move down (order for merge)"
              disabled={i + 1 === app.files.length}
              onclick={() => app.move(i, i + 1)}>↓</button
            >
            <button class="btn btn-ghost" onclick={() => app.open(file.path)}>Open</button>
          </div>
        {/each}
      </div>
      <div class="actions">
        {#each actions as action (action.label)}
          <button class="btn" title={action.hint} disabled={!idle || !action.ok} onclick={action.run}>
            {action.label}
          </button>
        {/each}
      </div>
    {/if}
  </section>

  <section class="card pane">
    <Pane {idle} />
  </section>
</div>

<style>
  .layout {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-rows: minmax(150px, auto) minmax(0, 1fr);
    gap: 12px;
  }

  .files {
    display: flex;
    flex-direction: column;
    min-height: 0;
    max-height: 45vh;
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

  .row,
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .list {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .all {
    padding: 8px 14px;
    border-bottom: 1px solid var(--border);
  }

  .file {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 10px 4px 14px;
    border-bottom: 1px solid var(--border);
  }

  .file.selected {
    background: var(--accent-soft);
  }

  .check.grow {
    flex: 1;
    min-width: 0;
    align-items: center;
  }

  .check.grow input {
    margin-top: 0;
  }

  .name {
    display: flex;
    align-items: baseline;
    gap: 10px;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
  }

  .name > * {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .actions {
    padding: 10px 14px;
  }

  .empty {
    padding: 28px 16px;
    text-align: center;
  }

  .empty p + p {
    margin-top: 4px;
  }

  .pane {
    min-height: 0;
    overflow: auto;
    padding: 14px 16px;
  }
</style>
