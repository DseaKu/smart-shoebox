<script lang="ts">
  // What the running job does, with a moving bar and Cancel (also Esc).
  let { what, onCancel }: { what: string; onCancel: () => void } = $props();
</script>

<div class="busy" role="status">
  <div class="line">
    <span class="what">{what}</span>
    <button class="btn" onclick={onCancel} title="Esc">Cancel</button>
  </div>
  <div class="bar"><div></div></div>
</div>

<style>
  .busy {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 16px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .line {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .what {
    flex: 1;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bar {
    height: 4px;
    border-radius: 999px;
    background: var(--surface-2);
    overflow: hidden;
  }

  .bar > div {
    width: 30%;
    height: 100%;
    border-radius: 999px;
    background: var(--accent);
    animation: slide 1.2s ease-in-out infinite;
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
    .bar > div {
      animation: none;
      width: 100%;
      opacity: 0.5;
    }
  }
</style>
