<script lang="ts">
  export let size = 16;
  export let onclick = () => {};
</script>

<span role="button" tabindex="0" onclick={onclick}>
  <slot />
</span>

<style>
  span {
    display: inline-block;
    transition: filter 0.15s ease;
  }

  span:hover {
    filter: drop-shadow(0 0 8px rgba(61, 93, 236, 0.993));
  }
</style>
