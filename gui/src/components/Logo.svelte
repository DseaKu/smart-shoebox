<script lang="ts">
  // The shoebox. Its papers bob while a job runs.
  let { busy }: { busy: boolean } = $props();
</script>

<svg class="logo" class:busy viewBox="0 0 32 32" width="26" height="26" aria-hidden="true">
  <g class="papers">
    <rect x="9" y="5" width="11" height="10" rx="1.2" fill="currentColor" opacity="0.35" transform="rotate(-7 14 10)" />
    <rect x="12" y="4" width="11" height="11" rx="1.2" fill="currentColor" opacity="0.6" transform="rotate(5 17 9)" />
  </g>
  <path d="M6 14.5 H26 L24.8 26 A1.6 1.6 0 0 1 23.2 27.4 H8.8 A1.6 1.6 0 0 1 7.2 26 Z" fill="currentColor" />
  <rect x="5" y="13" width="22" height="3.6" rx="1.2" fill="currentColor" />
  <rect x="13.5" y="19.5" width="5" height="1.8" rx="0.9" fill="var(--surface)" />
</svg>

<style>
  .papers {
    transform-box: view-box;
  }

  .logo.busy .papers {
    animation: bob 0.9s ease-in-out infinite alternate;
  }

  @keyframes bob {
    to {
      transform: translateY(-2.5px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .logo.busy .papers {
      animation-duration: 3s;
    }
  }
</style>
