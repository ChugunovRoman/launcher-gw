<script lang="ts">
  import { _ } from "svelte-i18n";
  import { factionWarningKey, factionMessageDetail } from "../lib/factionSettings";

  let { manifest, warnings = [] } = $props<{ manifest: FactionBundleManifest; warnings?: string[] }>();
</script>

<div class="summary">
  {#if manifest.author}
    <span>{$_("app.factionSettings.author")}: {manifest.author}</span>
  {/if}
  {#if manifest.created_at}
    <span>{$_("app.factionSettings.createdAt")}: {new Date(manifest.created_at).toLocaleString()}</span>
  {/if}
  <span>{$_("app.factionSettings.summary.factions")}: {manifest.summary.factions_created}/{manifest.summary.factions_total}</span>
  {#if manifest.summary.custom_armament.length}
    <span>{$_("app.factionSettings.summary.armament")}: {manifest.summary.custom_armament.join(", ")}</span>
  {/if}
  {#if manifest.summary.custom_squad_sizes.length}
    <span>{$_("app.factionSettings.summary.squadSizes")}: {manifest.summary.custom_squad_sizes.join(", ")}</span>
  {/if}
  {#if manifest.summary.has_relations}<span>{$_("app.factionSettings.summary.relations")}</span>{/if}
  {#if manifest.summary.has_population}<span>{$_("app.factionSettings.summary.population")}</span>{/if}
  {#if manifest.summary.has_point_types}<span>{$_("app.factionSettings.summary.pointTypes")}</span>{/if}
</div>

{#if warnings.length}
  <ul class="warnings">
    {#each warnings as w}
      <li>
        {$_(factionWarningKey(w))}
        {#if factionMessageDetail(w)}: {factionMessageDetail(w)}{/if}
      </li>
    {/each}
  </ul>
{/if}

<style>
  .summary {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 0.9em;
    opacity: 0.85;
    margin: 10px 0;
    color: white;
  }
  .warnings {
    margin: 0 0 10px 0;
    padding-left: 20px;
    color: #f5c542;
    font-size: 0.9em;
  }
</style>
