<script lang="ts">
  let { children, value, style = "" } = $props();
</script>

<div class="scroll" style="height: calc(100vh - {value}px); {style}">
  {@render children?.()}
</div>

<style>
  .scroll {
    overflow-y: auto;
    -webkit-app-region: no-drag;
    padding-right: 20px;
  }
  .scroll::-webkit-scrollbar {
    width: 12px;
  }
  .scroll::-webkit-scrollbar-track {
    background: transparent;
  }
  .scroll::-webkit-scrollbar-thumb {
    background-color: rgba(61, 93, 236, 0.8);
    border-radius: 6px;
    border: 3px solid transparent;
    background-clip: content-box;
  }
  .scroll::-webkit-scrollbar-thumb:hover {
    background-color: rgba(61, 93, 236, 1);
  }
  .scroll::-webkit-scrollbar-button {
    display: none;
  }
</style>
