<script lang="ts">
  export let isButton = true;
  export let size = 16;
  export let onclick = () => {};
</script>

<span class:btn={isButton} role="button" tabindex="0" {onclick}>
  <slot />
</span>

<style>
  .btn {
    display: flex;
    align-items: center;
    justify-content: center;
    transition: filter 0.15s ease;
  }

  .btn:hover {
    filter: drop-shadow(0 0 10px rgba(61, 93, 236, 0.993));
    cursor: pointer;
  }
</style>
