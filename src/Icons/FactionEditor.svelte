<script lang="ts">
  import { _ } from "svelte-i18n";
  import Wrap from "./wrap.svelte";

  export let color = "#FFF";
  export let size = 16;
  export let onclick = () => {};

  // The knockout mask is referenced by id; keep it unique per instance so several icons on one
  // page never resolve to each other's <mask>.
  const maskId = `fe-knockout-${Math.random().toString(36).slice(2, 8)}`;

  // Central figure, shared by the visible layer and the knockout mask below.
  const centralHead = { cx: 385, cy: 395, r: 72 };
  const centralBody = "M285 575 C285 490 325 450 385 450 C445 450 485 490 485 575 V630 C430 650 340 650 285 630 Z";
</script>

<Wrap {onclick} {size}>
  <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 1024 1024">
    <title>{$_("app.menu.factionSettings")}</title>
    <defs>
      <!-- Enlarged silhouette of the central figure: fill + stroke gives an even outward offset,
           so the side figures get a uniform gap around it instead of a hand-traced edge. -->
      <mask id={maskId} maskUnits="userSpaceOnUse" x="0" y="0" width="1024" height="1024">
        <rect width="1024" height="1024" fill="#fff" />
        <g fill="#000" stroke="#000" stroke-width="56" stroke-linejoin="round">
          <circle cx={centralHead.cx} cy={centralHead.cy} r={centralHead.r} />
          <path d={centralBody} />
        </g>
      </mask>
    </defs>

    <!-- Document: one stroke of constant width, open on the right for the arrows, classic folded corner. -->
    <g fill="none" stroke={color} stroke-width="60" stroke-linecap="round" stroke-linejoin="round">
      <path d="M670 430 V285 L520 135 H150 Q110 135 110 175 V850 Q110 890 150 890 H630 Q670 890 670 850 V835" />
      <path d="M520 135 V285 H670" />
    </g>

    <!-- Side figures, drawn whole and knocked out around the central one. -->
    <g fill={color} mask="url(#{maskId})">
      <circle cx="245" cy="460" r="48" />
      <path d="M185 610 V555 C185 505 210 480 245 480 C280 480 305 505 305 555 V610 Z" />
      <circle cx="525" cy="460" r="48" />
      <path d="M465 610 V555 C465 505 490 480 525 480 C560 480 585 505 585 555 V610 Z" />
    </g>

    <!-- Central figure and the import/export arrows. -->
    <g fill={color}>
      <circle cx={centralHead.cx} cy={centralHead.cy} r={centralHead.r} />
      <path d={centralBody} />
      <path d="M655 535 H820 V500 L900 565 L820 630 V595 H655 Z" />
      <path d="M810 675 H645 V640 L565 705 L645 770 V735 H810 Z" />
    </g>
  </svg>
</Wrap>
