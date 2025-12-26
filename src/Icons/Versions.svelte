<script lang="ts">
  import { _ } from "svelte-i18n";
  import Wrap from "./wrap.svelte";

  export let color = "#FFF";
  export let size = 16;
  export let onclick = () => {};
</script>

<Wrap {onclick} {size}>
  <svg width={size} height={size} viewBox="0 0 64 64" xmlns="http://www.w3.org/2000/svg">
    <title>{$_("app.menu.versions")}</title>
    <!-- Минималистичные слои -->
    <rect x="10" y="24" width="34" height="6" rx="2" fill={color} opacity="0.4" />
    <rect x="14" y="32" width="34" height="6" rx="2" fill={color} opacity="0.7" />
    <rect x="18" y="40" width="34" height="6" rx="2" fill={color} />
    <!-- Маленькая V -->
    <path d="M20 50 L23 54 L26 50" stroke={color} stroke-width="1.5" fill="none" />
    <!-- Маленькие стрелки -->
    <path d="M48 16 A6 6 0 1 1 44 20" fill="none" stroke={color} stroke-width="1.5" />
    <path d="M44 18 L44 20 L46 20" fill="none" stroke={color} stroke-width="1.5" />
  </svg>
</Wrap>
