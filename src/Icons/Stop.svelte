<script lang="ts">
  import Wrap from "./wrap.svelte";

  export let color = "#FFF";
  export let size = 16;
  export let onclick = () => {};
</script>

<Wrap {onclick} {size}>
  <svg fill={color} width={size} height={size} viewBox="-2.5 0 32 32" version="1.1" xmlns="http://www.w3.org/2000/svg">
    <path
      d="M2 12C2 7.28595 2 4.92893 3.46447 3.46447C4.92893 2 7.28595 2 12 2C16.714 2 19.0711 2 20.5355 3.46447C22 4.92893 22 7.28595 22 12C22 16.714 22 19.0711 20.5355 20.5355C19.0711 22 16.714 22 12 22C7.28595 22 4.92893 22 3.46447 20.5355C2 19.0711 2 16.714 2 12Z"
      fill={color} />
  </svg>
</Wrap>
