<script lang="ts">
  // A drop zone that also opens a file dialog on click.
  // The drop itself comes from the window (see App.svelte), because only Tauri knows the file path.
  import type { Snippet } from "svelte";

  interface Props {
    title: string;
    hint: string;
    disabled?: boolean;
    onChoose: () => void;
    children?: Snippet;
  }
  let { title, hint, disabled = false, onChoose, children }: Props = $props();
</script>

<div class="drop">
  <button class="target" {disabled} onclick={onChoose}>
    <svg viewBox="0 0 24 24" width="28" height="28" aria-hidden="true">
      <path
        fill="none"
        stroke="currentColor"
        stroke-width="1.6"
        stroke-linecap="round"
        stroke-linejoin="round"
        d="M14 3H7a2 2 0 00-2 2v14a2 2 0 002 2h10a2 2 0 002-2V8l-5-5zM14 3v5h5M12 18v-6M9.5 14.5L12 12l2.5 2.5"
      />
    </svg>
    <span class="text">
      <strong>{title}</strong>
      <span class="small muted">{hint}</span>
    </span>
  </button>
  {#if children}<div class="extra">{@render children()}</div>{/if}
</div>

<style>
  .drop {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 76px;
    padding: 10px 14px 10px 4px;
    border: 1.5px dashed var(--border-strong);
    border-radius: var(--radius);
    background: var(--surface-2);
    transition: border-color 0.12s, background 0.12s;
  }

  .drop:has(.target:hover:not(:disabled)) {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .target {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 4px 10px;
    border: 0;
    background: none;
    color: var(--muted);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .target:hover:not(:disabled) {
    color: var(--accent);
  }

  .target:disabled {
    cursor: default;
    opacity: 0.7;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    color: var(--text);
  }

  .extra {
    display: flex;
    gap: 6px;
  }
</style>
