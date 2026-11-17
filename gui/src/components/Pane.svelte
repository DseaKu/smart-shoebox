<script lang="ts">
  // The result of the last action in the Files tab: names, a sort plan, a check,
  // counterparts, or a written result with its originals.
  import { app } from "../lib/app.svelte";
  import * as api from "../lib/api";

  let { idle }: { idle: boolean } = $props();

  let pane = $derived(app.pane);

  // For several checked files: the fixes that work on all of them at once.
  let ok = $derived(pane.kind === "check" ? pane.results.filter((r) => !r.error) : []);
  let unnamed = $derived(ok.filter((r) => !r.readable_name).map((r) => r.file));
  let notInBox = $derived(ok.filter((r) => !r.in_box).map((r) => r.file));
</script>

{#if pane.kind === "empty"}
  <div class="empty">
    <p>Select files and choose an action.</p>
    <p class="muted small">The result shows here.</p>
  </div>
{:else if pane.kind === "names"}
  <h2>Names</h2>
  {#each pane.items as item (item.file)}
    <div class="block">
      <span class="muted small">{api.basename(item.file)}</span>
      {#if item.error}<p class="error-text small">{item.error}</p>{/if}
      {#if item.done}
        <p class="ok-text small">{item.done}</p>
      {:else}
        <div class="row">
          <div class="suffix">
            <input type="text" bind:value={item.stem} onkeydown={(e) => e.key === "Enter" && app.rename(item)} />
            <span class="muted">.pdf</span>
          </div>
          <button class="btn btn-primary" onclick={() => app.rename(item)}>Rename</button>
          <button class="btn" onclick={() => (item.done = "not renamed")}>Skip</button>
        </div>
      {/if}
    </div>
  {/each}
  <div class="footer"><button class="btn" onclick={() => app.closePane()}>Done</button></div>
{:else if pane.kind === "sort"}
  {@const layout = pane.layout}
  <h2>{pane.view.heading}</h2>
  {#each layout.documents as doc, g (g)}
    <div class="block">
      <h3>Document {g + 1}: {doc.title}</h3>
      <table class="pages">
        <tbody>
          {#each doc.pages as page (page.number)}
            <tr>
              <td class="num">page {page.number}</td>
              <td class="num">{#if page.printed}<span class="chip">{page.printed}</span>{/if}</td>
              <td class="muted small preview">{page.preview}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/each}
  {#each layout.missing as note}<p class="mid-text">{note}</p>{/each}
  {#if layout.duplicates.length > 0}
    <p class="muted small">Duplicate scans, left out: {layout.duplicates.join(", ")}</p>
  {/if}
  <div class="block">
    <label class="field">
      <span>Page order</span>
      <input
        type="text"
        class="mono"
        value={pane.order}
        oninput={(e) => app.editOrder(e.currentTarget.value)}
        spellcheck="false"
      />
      <span class="hint">';' separates documents; blank pages and duplicates may be left out.</span>
    </label>
    {#if layout.error}<p class="error-text small">{layout.error}</p>{/if}
    {#if layout.blank.length > 0}
      <label class="check">
        <input type="checkbox" bind:checked={pane.deleteBlank} />
        <span>Delete blank page(s) {layout.blank.join(", ")} (otherwise they go to the end)</span>
      </label>
    {/if}
  </div>
  <div class="footer">
    <button class="btn btn-primary" disabled={!!layout.error} onclick={() => app.writeSort()}>
      Write {pane.view.out_name}
    </button>
    <button class="btn" onclick={() => app.closePane()}>Cancel</button>
  </div>
{:else if pane.kind === "check"}
  <h2>{pane.results.length === 1 ? `Check ${pane.results[0].name}` : `Check of ${pane.results.length} files`}</h2>
  {#each pane.results as result (result.file)}
    <div class="block">
      {#if pane.results.length > 1}<strong>{result.name}</strong>{/if}
      {#if result.error}
        <p class="error-text">{result.error}</p>
      {:else}
        <pre class="report">{result.lines.join("\n")}</pre>
        <div class="row">
          {#if result.incomplete}
            <button
              class="btn"
              disabled={!idle}
              title="Look for the missing pages in the other PDFs of its folder"
              onclick={() => app.counterpart(result.file)}>Search counterpart</button
            >
          {/if}
          {#if !result.readable_name}
            <button class="btn" disabled={!idle} onclick={() => app.name([result.file])}>Name it</button>
          {/if}
          <button class="btn" disabled={!idle} onclick={() => app.sort(result.file)}>Sort it</button>
          {#if !result.in_box}
            <button class="btn" onclick={() => app.queueForBox([result.file])}>Add to box</button>
          {/if}
        </div>
      {/if}
    </div>
  {/each}
  <div class="footer">
    {#if pane.results.length > 1 && unnamed.length > 1}
      <button class="btn" disabled={!idle} onclick={() => app.name(unnamed)}>Name all {unnamed.length} unreadable</button>
    {/if}
    {#if pane.results.length > 1 && notInBox.length > 1}
      <button class="btn" onclick={() => app.queueForBox(notInBox)}>Add all {notInBox.length} to the box</button>
    {/if}
    <button class="btn" onclick={() => app.closePane()}>Close</button>
  </div>
{:else if pane.kind === "counterparts"}
  {@const found = pane.found}
  <h2>Counterparts for {found.name}</h2>
  <p class="muted small">Searched {found.dir}</p>
  {#if found.candidates.length === 0}<p>None found.</p>{/if}
  {#each found.candidates as candidate, i (candidate.path)}
    <div class="block">
      <strong>{candidate.name}</strong>
      <p>{candidate.description}</p>
      {#if candidate.still}<p class="mid-text small">Still missing afterwards: {candidate.still}</p>{/if}
      <div class="row">
        <button class="btn btn-primary" disabled={!idle} onclick={() => app.useCandidate(i)}>
          Merge and sort with this
        </button>
        <button class="btn btn-ghost" onclick={() => app.open(candidate.path)}>Open</button>
      </div>
    </div>
  {/each}
  {#if found.rejected.length > 0}
    <details class="block" open={found.candidates.length === 0}>
      <summary class="small muted">Not suggested ({found.rejected.length})</summary>
      <ul class="rejected">
        {#each found.rejected as r (r.path)}
          <li><strong>{r.name}</strong> <span class="muted">{r.reason}</span></li>
        {/each}
      </ul>
    </details>
  {/if}
  <div class="footer"><button class="btn" onclick={() => app.closePane()}>Close</button></div>
{:else if pane.kind === "done"}
  <h2>{pane.heading}</h2>
  <div class="block">
    <span class="muted small">From:</span>
    {#each pane.originals as file (file)}<p>{api.basename(file)}</p>{/each}
  </div>
  <div class="footer">
    {#if pane.offerTrash}
      <button class="btn btn-danger" onclick={() => app.trashOriginals()}>Move the originals to the Trash</button>
    {/if}
    <button class="btn" onclick={() => app.closePane()}>Keep them</button>
  </div>
{/if}

<style>
  h2 {
    margin-bottom: 4px;
  }

  .block {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px 0;
    border-bottom: 1px solid var(--border);
  }

  .row,
  .footer {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }

  .footer {
    padding-top: 12px;
  }

  .suffix {
    flex: 1;
    min-width: 200px;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .pages {
    width: 100%;
    border-collapse: collapse;
  }

  .pages td {
    padding: 3px 8px 3px 0;
    vertical-align: top;
  }

  .pages .num {
    width: 1%;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .preview {
    overflow-wrap: anywhere;
  }

  .mono,
  .report {
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  }

  .report {
    margin: 0;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    font-size: 12px;
    white-space: pre-wrap;
  }

  summary {
    cursor: pointer;
    user-select: none;
  }

  .rejected {
    margin: 6px 0 0;
    padding-left: 18px;
  }

  .rejected li + li {
    margin-top: 4px;
  }

  .empty {
    padding: 40px 16px;
    text-align: center;
  }

  .empty p + p {
    margin-top: 4px;
  }
</style>
