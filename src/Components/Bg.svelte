<script lang="ts">
  let { children, style = "" } = $props();
</script>

<div class="bg-item" {style}>
  {@render children?.()}
</div>

<style>
  .bg-item {
    padding: 12px 12px;
    -webkit-app-region: no-drag;
    background-color: rgba(40, 40, 40, 0.6);
    border-radius: 6px;
    margin-bottom: 1rem;
    overflow: hidden;
    transition: background-color 0.2s ease;
  }
  .bg-item:hover {
    background-color: rgba(50, 50, 50, 0.7);
  }
</style>
