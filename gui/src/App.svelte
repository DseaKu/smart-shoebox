<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { app, type Tab } from "./lib/app.svelte";
  import Logo from "./components/Logo.svelte";
  import BoxChooser from "./components/BoxChooser.svelte";
  import SetupScreen from "./components/SetupScreen.svelte";
  import BusyBar from "./components/BusyBar.svelte";
  import WizardTab from "./components/WizardTab.svelte";
  import FilesTab from "./components/FilesTab.svelte";
  import FindTab from "./components/FindTab.svelte";
  import AskTab from "./components/AskTab.svelte";
  import AddTab from "./components/AddTab.svelte";

  let ready = $state(false);
  let dragging = $state(false);
  let choosing = $state(false);

  const tabs: { id: Tab; label: string }[] = [
    { id: "wizard", label: "Wizard" },
    { id: "files", label: "Files" },
    { id: "find", label: "Find" },
    { id: "ask", label: "Ask" },
    { id: "add", label: "Add" },
  ];

  let dropHint = $derived(app.tab === "wizard" ? "Drop PDFs or folders for the wizard" : "Drop PDFs to open them in Files");

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && app.busy) app.cancel();
  }

  onMount(() => {
    const unlisten = [
      listen<string>("wizard-log", (e) => {
        app.wizardLog.push(e.payload);
      }),
      getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        if (!app.box || choosing || app.needsSetup) return;
        if (p.type === "enter" || p.type === "over") dragging = true;
        else if (p.type === "leave") dragging = false;
        else if (p.type === "drop") {
          dragging = false;
          if (p.paths.length > 0) app.dropped(p.paths);
        }
      }),
    ];
    app.loadBox().then(() => (ready = true));
    return () => unlisten.forEach((p) => p.then((stop) => stop()));
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="app">
  <header>
    <div class="brand">
      <Logo busy={!!app.busy} />
      <h1>Smart Shoebox</h1>
    </div>
    {#if app.box && !choosing && !app.needsSetup}
      <nav aria-label="Sections">
        {#each tabs as tab (tab.id)}
          <button class="tab" class:active={app.tab === tab.id} onclick={() => (app.tab = tab.id)}>
            {tab.label}
            {#if tab.id === "add" && app.addCount > 0}<span class="badge">{app.addCount}</span>{/if}
          </button>
        {/each}
      </nav>
      <div class="where">
        <span class="muted small path" title={app.box.root}><bdi>{app.box.root}</bdi></span>
        {#if app.setup && !app.setup.ready}
          <button class="btn btn-ghost mid-text" onclick={() => (app.setupSkipped = false)} disabled={!!app.busy}>
            AI not ready
          </button>
        {:else}
          <span class="chip small">{app.box.model}</span>
        {/if}
        <button class="btn btn-ghost" onclick={() => (choosing = true)} disabled={!!app.busy}>Change box</button>
      </div>
    {/if}
  </header>

  {#if !ready}
    <main></main>
  {:else if !app.box || choosing}
    <BoxChooser problem={app.boxProblem} onCancel={app.box ? () => (choosing = false) : null} onOpened={() => (choosing = false)} />
  {:else if app.needsSetup && app.setup}
    <SetupScreen setup={app.setup} />
  {:else}
    {#if app.busy}
      <BusyBar what={app.busy} onCancel={() => app.cancel()} />
    {/if}
    <main>
      {#if app.notice}
        <div class="notice" class:error={app.notice.kind === "error"}>
          <span class="grow">{app.notice.text}</span>
          <button class="btn btn-ghost btn-icon" aria-label="Close" onclick={() => (app.notice = null)}>✕</button>
        </div>
      {/if}
      <div class="body">
        {#if app.tab === "wizard"}
          <WizardTab />
        {:else if app.tab === "files"}
          <FilesTab />
        {:else if app.tab === "find"}
          <FindTab />
        {:else if app.tab === "ask"}
          <AskTab />
        {:else}
          <AddTab />
        {/if}
      </div>
    </main>
    {#if dragging}
      <div class="drop-overlay" aria-hidden="true"><div>{dropHint}</div></div>
    {/if}
  {/if}
</div>

<style>
  .app {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  header {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    color: var(--accent);
  }

  .brand h1 {
    font-size: 17px;
    color: var(--text);
    white-space: nowrap;
  }

  nav {
    display: flex;
    gap: 2px;
    padding: 3px;
    border-radius: var(--radius);
    background: var(--surface-2);
  }

  .tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-weight: 500;
    cursor: pointer;
  }

  .tab:hover {
    color: var(--text);
  }

  .tab.active {
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow);
  }

  .badge {
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--on-accent);
    font-size: 11px;
    line-height: 18px;
    text-align: center;
  }

  .where {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
  }

  .path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }

  main {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px;
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .drop-overlay {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 24px;
    background: color-mix(in srgb, var(--bg) 70%, transparent);
    pointer-events: none;
  }

  .drop-overlay div {
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
    border: 2px dashed var(--accent);
    border-radius: var(--radius);
    background: var(--accent-soft);
    color: var(--accent);
    font-size: 16px;
    font-weight: 600;
  }

  @media (max-width: 860px) {
    header {
      flex-wrap: wrap;
    }

    .where {
      flex-basis: 100%;
    }
  }
</style>
