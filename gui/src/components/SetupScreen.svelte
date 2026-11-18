<script lang="ts">
  // Shown while the AI is not ready: Ollama is missing or stopped, or a model of
  // the box is not downloaded. Ollama is installed from its official page; the
  // app starts it and downloads the models.
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import * as api from "../lib/api";
  import { app } from "../lib/app.svelte";
  import type { PullProgress, SetupState } from "../lib/types";

  let { setup }: { setup: SetupState } = $props();

  let answers = $derived(setup.models.find((m) => m.purpose === "answers"));
  let others = $derived(setup.models.filter((m) => m.purpose !== "answers"));
  let running = $derived(setup.ollama === "running");

  /** The model for answers that I picked; starts with the box's model or the suggestion. */
  let picked = $state("");
  $effect.pre(() => {
    if (picked || !answers) return;
    const known = setup.choices.some((c) => c.name === answers.name);
    picked = answers.installed || !known ? answers.name : setup.suggested;
  });

  let choices = $derived.by(() => {
    const list = setup.choices.map((c) => ({ ...c }));
    if (answers && !list.some((c) => c.name === answers.name)) list.push({ name: answers.name, size: answers.size ?? "" });
    return list;
  });

  let starting = $state(false);
  let progress = $state<PullProgress | null>(null);
  let error = $state<string | null>(null);

  /** The download size still missing, for the button. */
  let missingSize = $derived.by(() => {
    const sizes = setup.models
      .filter((m) => !m.installed && m.purpose !== "answers")
      .map((m) => m.size)
      .concat(answers && (picked !== answers.name || !answers.installed) ? [choices.find((c) => c.name === picked)?.size ?? null] : []);
    if (sizes.length === 0) return "";
    if (sizes.some((s) => !s)) return "";
    const gb = sizes.reduce((sum, s) => sum + parseFloat(s!), 0);
    return `${gb.toFixed(1)} GB`;
  });

  async function getOllama() {
    error = null;
    await app.quick(api.openOllamaDownload);
  }

  async function start() {
    error = null;
    starting = true;
    try {
      await api.startOllama();
    } catch (e) {
      error = api.errorText(e);
    } finally {
      starting = false;
      await app.checkSetup();
    }
  }

  async function download() {
    error = null;
    if (answers && picked !== answers.name) {
      try {
        app.box = await api.setModel(picked);
      } catch (e) {
        error = api.errorText(e);
        return;
      }
      await app.checkSetup();
    }
    for (const model of app.setup?.models.filter((m) => !m.installed) ?? []) {
      progress = { model: model.name, status: "starting", completed: 0, total: 0 };
      const ok = await app.job(`Downloading ${model.name}…`, () => api.pullModel(model.name).then(() => true));
      if (!ok) break;
    }
    progress = null;
    await app.checkSetup();
  }

  /** Ollama's status ("pulling a3de86cd1c13", "verifying sha256 digest") in plain words. */
  function plain(status: string) {
    if (status === "pulling manifest" || status === "starting") return "preparing";
    if (status.startsWith("pulling")) return "downloading";
    if (status.startsWith("verifying")) return "checking";
    if (status === "writing manifest") return "finishing";
    if (status === "success") return "done";
    return status;
  }

  function size(bytes: number) {
    return bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
  }

  let percent = $derived(progress && progress.total > 0 ? Math.round((progress.completed / progress.total) * 100) : null);

  onMount(() => {
    const stop = listen<PullProgress>("pull-progress", (e) => {
      if (progress) progress = e.payload;
    });
    // While Ollama is not running, look again every few seconds, so the screen
    // continues by itself after the install.
    const timer = setInterval(() => {
      if (!running && !starting && !app.busy) app.checkSetup();
    }, 3000);
    return () => {
      clearInterval(timer);
      stop.then((f) => f());
    };
  });
</script>

<main>
  <section class="card setup">
    <h2>Set up the AI</h2>
    <p class="muted">
      Smart Shoebox reads, names and answers with an AI that runs on this computer. Your documents never leave it. This
      takes two steps, once.
    </p>

    {#if error}<p class="notice error">{error}</p>{/if}
    {#if app.notice?.kind === "error"}<p class="notice error">{app.notice.text}</p>{/if}

    <ol class="steps">
      <li class:done={running}>
        <div class="step-head">
          <span class="num">{running ? "✓" : "1"}</span>
          <h3>Ollama</h3>
          {#if running}<span class="ok-text small">is running</span>{/if}
        </div>
        {#if setup.ollama === "missing"}
          <p>Ollama is the free program that runs the AI. Install it like any other app:</p>
          <ol class="how">
            <li>Click <b>Get Ollama</b>. Your browser opens the Ollama website.</li>
            <li>Click <b>Download manually</b>. Open the downloaded file and install Ollama.</li>
            <li>Come back to this window. It continues by itself.</li>
          </ol>
          <div class="actions">
            <button class="btn btn-primary btn-big" onclick={getOllama}>Get Ollama</button>
            <span class="muted small waiting">Waiting for Ollama…</span>
          </div>
        {:else if setup.ollama === "stopped"}
          <p>Ollama is installed, but it is not running.</p>
          <div class="actions">
            <button class="btn btn-primary btn-big" onclick={start} disabled={starting}>
              {starting ? "Starting Ollama…" : "Start Ollama"}
            </button>
          </div>
        {/if}
      </li>

      <li class:inactive={!running}>
        <div class="step-head">
          <span class="num">2</span>
          <h3>AI models</h3>
        </div>
        <p class="muted small">
          Choose the model for answers. Bigger models are better, but slower and need more memory.
          {#if setup.ram_gb > 0}This computer has {setup.ram_gb} GB.{/if}
        </p>
        <div class="choices" role="radiogroup" aria-label="Model for answers">
          {#each choices as choice (choice.name)}
            <label class="choice" class:picked={picked === choice.name}>
              <input type="radio" name="model" value={choice.name} bind:group={picked} disabled={!running || !!progress} />
              <span class="name">{choice.name}</span>
              <span class="muted small">{choice.size}</span>
              {#if choice.name === setup.suggested}<span class="chip small">suggested</span>{/if}
              {#if answers && choice.name === answers.name && answers.installed}<span class="ok-text small">downloaded</span>{/if}
            </label>
          {/each}
        </div>
        {#each others as model (model.name)}
          <p class="small">
            <span class="name">{model.name}</span>
            <span class="muted">{model.size ?? ""}, for searching inside documents</span>
            {#if model.installed}<span class="ok-text">downloaded</span>{/if}
          </p>
        {/each}

        {#if progress}
          <div class="progress" aria-live="polite">
            <div class="bar"><div style:width="{percent ?? 0}%" class:moving={percent === null}></div></div>
            <div class="small progress-text">
              <span><b>{progress.model}</b>: {plain(progress.status)}</span>
              {#if progress.total > 0}<span class="muted">{size(progress.completed)} of {size(progress.total)}</span>{/if}
            </div>
            <div class="actions">
              <button class="btn" onclick={() => app.cancel()}>Cancel</button>
              <span class="muted small">After a cancel, the next download continues where this one stopped.</span>
            </div>
          </div>
        {:else}
          <div class="actions">
            <button class="btn btn-primary btn-big" onclick={download} disabled={!running}>
              Download{missingSize ? ` (${missingSize})` : ""}
            </button>
          </div>
        {/if}
      </li>
    </ol>

    <button class="btn btn-ghost skip" onclick={() => (app.setupSkipped = true)} disabled={!!progress}>
      Skip for now (Find, Ask and the Wizard need the AI)
    </button>
  </section>
</main>

<style>
  main {
    flex: 1;
    min-height: 0;
    overflow: auto;
    display: grid;
    place-items: start center;
    padding: 24px 16px;
  }

  .setup {
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: 100%;
    max-width: 600px;
    padding: 28px;
  }

  h2 {
    font-size: 18px;
  }

  h3 {
    font-size: 15px;
  }

  .steps {
    display: flex;
    flex-direction: column;
    gap: 14px;
    margin: 4px 0 0;
    padding: 0;
    list-style: none;
  }

  .steps > li {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .steps > li.inactive {
    opacity: 0.55;
  }

  .steps > li.done {
    background: var(--good-soft);
    border-color: transparent;
  }

  .step-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .num {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
    font-size: 13px;
  }

  .done .num {
    background: var(--good);
    color: var(--on-accent);
  }

  .how {
    margin: 0;
    padding-left: 20px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    margin-top: 4px;
  }

  .waiting::before {
    content: "";
    display: inline-block;
    width: 8px;
    height: 8px;
    margin-right: 6px;
    border-radius: 999px;
    background: var(--mid);
    animation: pulse 1.4s ease-in-out infinite;
  }

  .choices {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .choice {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .choice.picked {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .choice input {
    accent-color: var(--accent);
    margin: 0;
  }

  .name {
    font-weight: 500;
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
    font-size: 13px;
  }

  p {
    margin: 0;
  }

  .progress {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .bar {
    height: 8px;
    border-radius: 999px;
    background: var(--surface-2);
    overflow: hidden;
  }

  .bar > div {
    height: 100%;
    background: var(--accent);
    transition: width 0.3s;
  }

  .bar > div.moving {
    width: 30% !important;
    animation: slide 1.2s ease-in-out infinite;
  }

  .progress-text {
    display: flex;
    justify-content: space-between;
    gap: 10px;
  }

  .skip {
    align-self: center;
    color: var(--muted);
    white-space: normal;
    height: auto;
    min-height: 32px;
  }

  @keyframes pulse {
    50% {
      opacity: 0.3;
    }
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .waiting::before,
    .bar > div.moving {
      animation: none;
    }
  }
</style>
