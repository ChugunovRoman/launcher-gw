<script lang="ts">
  import { _ } from "svelte-i18n";
  import Wrap from "./wrap.svelte";

  export let isButton = true;
  export let color = "#FFF";
  export let size = 16;
  export let onclick = () => {};
</script>

<Wrap {onclick} {size} {isButton}>
  <svg width="100" height="100" viewBox="0 0 22 22" xmlns="http://www.w3.org/2000/svg">
    <path
      xmlns="http://www.w3.org/2000/svg"
      d="M8 11L12 15M12 15L16 11M12 15V3M21 11V17.7992C21 18.9193 21 19.4794 20.782 19.9072C20.5903 20.2835 20.2843 20.5895 19.908 20.7812C19.4802 20.9992 18.9201 20.9992 17.8 20.9992H6.2C5.0799 20.9992 4.51984 20.9992 4.09202 20.7812C3.71569 20.5895 3.40973 20.2835 3.21799 19.9072C3 19.4794 3 18.9193 3 17.7992V11"
      fill="none"
      stroke="white"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round" />
  </svg>
</Wrap>
