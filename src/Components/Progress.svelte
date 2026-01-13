<script lang="ts">
  let {
    progress = $bindable(0),
    width = $bindable<number | undefined>(),
    maxWidth = $bindable<string | undefined>(),
    height = $bindable<number | undefined>(),
    showPercents = true,
    style = "",
  } = $props();
</script>

<div class="progress-container" style="width: {width}px; max-width: {maxWidth}; height: {height}px; {style}">
  <div class="progress-bar" style="width: {Math.min(100, Math.max(0, Number(progress.toFixed(2))))}%;"></div>
  {#if showPercents}
    <span class="progress-text">{Number(progress.toFixed(2))}%</span>
  {/if}
</div>

<style>
  .progress-container {
    width: 100%;
    height: 24px;
    background-color: #e0e0e0;
    border-radius: 4px;
    overflow: hidden;
    position: relative;
    margin: 8px 0;
  }
  .progress-bar {
    height: 100%;
    background: linear-gradient(to right, #2196f3, #4caf50);
    transition: width 0.2s ease;
  }
  .progress-text {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    font-size: 14px;
    font-weight: 500;
    color: #333;
    text-shadow:
      0 0 2px #fff,
      0 0 2px #fff;
  }
</style>
