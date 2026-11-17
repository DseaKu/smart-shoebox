<script lang="ts">
  // Drop a pile of PDFs; each is checked, completed, named and added without
  // questions, and I see what is left for me.
  import { open } from "@tauri-apps/plugin-dialog";
  import { app } from "../lib/app.svelte";
  import * as api from "../lib/api";
  import DropZone from "./DropZone.svelte";
  import Log from "./Log.svelte";

  let idle = $derived(!app.busy);
  let summary = $derived(app.wizard);
  let needsYou = $derived(summary?.items.filter((i) => i.status === "needs_you") ?? []);
  let added = $derived(summary?.items.filter((i) => i.status === "added") ?? []);
  let rest = $derived(summary?.items.filter((i) => i.status === "already_in_box" || i.status === "skipped") ?? []);

  async function choosePdfs() {
    const paths = await open({ multiple: true, filters: [{ name: "PDF", extensions: ["pdf", "PDF"] }] });
    if (paths) app.addWizardInputs(paths);
  }

  async function chooseFolder() {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === "string") app.addWizardInputs([dir]);
  }

  function openInFiles() {
    app.openFiles(needsYou.map((i) => i.file));
  }
</script>

<div class="scroll">
  <section class="card intro">
    <div>
      <h2>Drop your PDFs, the wizard does the rest</h2>
      <p class="muted small">
        Each PDF is checked; an incomplete scan is completed with its counterpart from the same pile and sorted; an
        unreadable name gets a readable one; then it is added to the box. Anything it cannot decide safely is left for
        you. Nothing is deleted.
      </p>
    </div>
    <div class="pick">
      <DropZone title="Drop PDFs or folders here" hint="or click to choose PDFs" disabled={!idle} onChoose={choosePdfs}>
        <button class="btn" onclick={chooseFolder} disabled={!idle}>Choose a folder…</button>
      </DropZone>
    </div>
    {#if app.wizardInputs.length > 0}
      <ul class="inputs">
        {#each app.wizardInputs as input, i (input.path)}
          <li>
            <span class="kind">{input.kind === "folder" ? "Folder" : "PDF"}</span>
            <span class="name" title={input.path}>{input.path}</span>
            <button
              class="btn btn-ghost btn-icon"
              aria-label="Remove"
              disabled={!idle}
              onclick={() => app.wizardInputs.splice(i, 1)}>✕</button
            >
          </li>
        {/each}
      </ul>
      <div class="row">
        <button class="btn btn-primary btn-big" onclick={() => app.startWizard()} disabled={!idle}>Start</button>
        <button class="btn btn-ghost" onclick={() => (app.wizardInputs = [])} disabled={!idle}>Clear</button>
      </div>
    {/if}
  </section>

  {#if !summary && app.wizardLog.length > 0}
    <section class="card pad">
      <h3 class="section-title">Progress</h3>
      <Log lines={app.wizardLog} follow={!idle} open />
    </section>
  {/if}

  {#if summary}
    {#if needsYou.length > 0}
      <section class="card pad">
        <div class="head">
          <h2>Needs you <span class="count">{needsYou.length}</span></h2>
          <button class="btn" onclick={openInFiles} title="To name, sort, check or add them yourself"
            >Open these in Files</button
          >
        </div>
        {#each needsYou as item (item.file)}
          <div class="item">
            <strong>{item.name}</strong>
            <p class="mid-text">{item.reason}</p>
            {#each item.steps as step}<p class="muted small">done: {step}</p>{/each}
          </div>
        {/each}
      </section>
    {/if}

    {#if added.length > 0}
      <section class="card pad">
        <h2>Added to the box <span class="count">{added.length}</span></h2>
        {#each added as item (item.file)}
          <div class="item doc">
            <span class="date">{item.entry?.date}</span>
            <div>
              <strong>{item.entry?.summary}</strong>
              <p class="muted small">{item.entry?.tags} · {item.name}</p>
              {#each item.steps as step}<p class="muted small">{step}</p>{/each}
            </div>
          </div>
        {/each}
        {#if summary.spare_originals.length > 0}
          <div class="notice spare">
            <span class="grow">
              The scans {summary.spare_originals.map(api.basename).join(", ")} were merged into documents that are now
              in the box.
            </span>
            <button class="btn" onclick={() => app.trashSpareOriginals()} disabled={!idle}>Move them to the Trash</button>
          </div>
        {/if}
      </section>
    {/if}

    {#if rest.length > 0}
      <section class="card pad">
        <h2>Nothing to do <span class="count">{rest.length}</span></h2>
        {#each rest as item (item.file)}<p class="muted small">{item.line}</p>{/each}
      </section>
    {/if}

    <section class="card pad">
      <Log lines={app.wizardLog} />
    </section>
  {/if}
</div>

<style>
  .scroll {
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow: auto;
    min-height: 0;
  }

  .intro {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px;
  }

  .intro p {
    margin-top: 4px;
    max-width: 75ch;
  }

  .pick {
    display: flex;
  }

  .inputs {
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .inputs li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 2px 4px 2px 10px;
  }

  .inputs li + li {
    border-top: 1px solid var(--border);
  }

  .kind {
    width: 52px;
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .pad {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .count {
    margin-left: 4px;
    color: var(--muted);
    font-weight: 500;
  }

  .item {
    padding-top: 10px;
    border-top: 1px solid var(--border);
  }

  .doc {
    display: flex;
    gap: 14px;
  }

  .spare {
    align-items: center;
  }
</style>
