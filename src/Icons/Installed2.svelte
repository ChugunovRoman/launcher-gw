<script lang="ts">
  import Wrap from "./wrap.svelte";

  export let isButton = true;
  export let color = "#FFF";
  export let size = 16;
  export let onclick = () => {};
</script>

<Wrap {onclick} {size} {isButton}>
  <svg width={size} height={size} viewBox="0 0 48 48" xmlns="http://www.w3.org/2000/svg">
    <path d="M10 28 L20 38 L38 16" stroke="#10B981" stroke-width="8" stroke-linecap="round" fill="none" />
  </svg>
</Wrap>
