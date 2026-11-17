<script lang="ts">
  // Shown when no box is open (first start), or to change the box.
  import { ask, open } from "@tauri-apps/plugin-dialog";
  import * as api from "../lib/api";
  import { app } from "../lib/app.svelte";
  import type { BoxInfo } from "../lib/types";

  interface Props {
    problem: string | null;
    onCancel: (() => void) | null;
    onOpened: () => void;
  }
  let { problem, onCancel, onOpened }: Props = $props();

  let error = $state<string | null>(null);

  async function choose() {
    error = null;
    const dir = await open({ directory: true, multiple: false, title: "Choose your shoebox" });
    if (typeof dir !== "string") return;
    try {
      let info: BoxInfo | null = await api.openBox(dir);
      if (!info) {
        const create = await ask(`There is no shoebox in ${dir}. Create one here?`, {
          title: "Smart Shoebox",
          kind: "info",
        });
        if (!create) return;
        info = await api.createBox(dir);
      }
      await app.useBox(info);
      onOpened();
    } catch (e) {
      error = api.errorText(e);
    }
  }
</script>

<main>
  <section class="card chooser">
    <svg viewBox="0 0 24 24" width="40" height="40" aria-hidden="true">
      <path
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
        stroke-linecap="round"
        stroke-linejoin="round"
        d="M3 7.5h18M4.5 7.5l1 11.2a2 2 0 002 1.8h9a2 2 0 002-1.8l1-11.2M3 4.5h18v3H3zM10 12h4"
      />
    </svg>
    <h2>Choose your shoebox</h2>
    <p class="muted">
      Choose the folder of your shoebox (the one with <code>shoebox.yaml</code>), or an empty folder to create a new
      one. The app remembers it for next time.
    </p>
    {#if problem}<p class="notice error">{problem}</p>{/if}
    {#if error}<p class="notice error">{error}</p>{/if}
    <div class="actions">
      <button class="btn btn-primary btn-big" onclick={choose}>Choose a folder…</button>
      {#if onCancel}<button class="btn btn-big" onclick={onCancel}>Cancel</button>{/if}
    </div>
  </section>
</main>

<style>
  main {
    flex: 1;
    display: grid;
    place-items: center;
    padding: 24px;
  }

  .chooser {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    max-width: 460px;
    padding: 32px;
    text-align: center;
  }

  svg {
    color: var(--accent);
  }

  h2 {
    font-size: 18px;
  }

  .actions {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
</style>
