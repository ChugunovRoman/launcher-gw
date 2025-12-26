<script lang="ts">
  import Wrap from "./wrap.svelte";

  export let isButton = true;
  export let color = "#FFF";
  export let size = 16;
  export let onclick = () => {};
</script>

<Wrap {onclick} {size} {isButton}>
  <svg width={size} height={size} viewBox="0 0 48 48" xmlns="http://www.w3.org/2000/svg">
    <path
      d="
    M24,4
    A20,20 0 1,1 24,44
    A20,20 0 1,1 24,4
    Z
    M16,26
    L22,32
    L34,20
    Q34,20 33,19
    Q32,18 31,19
    L22,28
    L17,23
    Q16,22 15,23
    Q14,24 15,25
    L16,26
    Z
  "
      fill="#10B981"
      fill-rule="evenodd" />
  </svg>
</Wrap>
