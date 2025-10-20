<script lang="ts">
  import { _ } from "svelte-i18n";
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";

  let coping = $state(false);
  let coping2 = $state(false);
  let uuid = $state("");

  async function copyUuid() {
    await writeText(uuid);

    coping = true;
    setTimeout(() => (coping2 = true), 500);
    setTimeout(() => (coping = false), 1000);
    setTimeout(() => (coping2 = false), 1500);
  }

  onMount(async () => {
    const config = await invoke<AppConfig>("get_config");

    uuid = config.client_uuid;
  });
</script>

<div class="settings_view">
  <h2>{$_("app.labels.settings")}</h2>

  <div class="input-row">
    <input type="text" readonly bind:value={uuid} placeholder="" class="uuid-input" />
    <button type="button" onclick={copyUuid} class="copy-btn" class:copy-btn__coping={coping} class:long_t={coping2}>
      {#if coping}
        {$_("app.copy.2")}
      {:else}
        {$_("app.copy.1")}
      {/if}
    </button>
  </div>
</div>

<style>
  h2 {
    margin-bottom: 4rem;
  }
  .settings_view {
    padding: 1.5rem;
    margin: 0 auto;
    font-family: system-ui, sans-serif;
  }
  .input-row {
    -webkit-app-region: no-drag;
    display: flex;
    gap: 0.75rem;
    margin-bottom: 2.5rem;
  }
  .uuid-input {
    -webkit-app-region: no-drag;
    flex: 1;
    padding: 0.5rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    background-color: rgba(255, 255, 255, 0.8);
  }
  .uuid-input:focus {
    background-color: rgba(255, 255, 255, 1);
  }
  .copy-btn {
    -webkit-app-region: no-drag;
    padding: 0.6rem 1.6rem;
    color: #fff;
    background-color: rgba(61, 93, 236, 0.8);
    border: 0px solid #ccc;
    border-radius: 3px;
    cursor: pointer;
    transition: background-color 0.15s ease;
  }
  .copy-btn:hover {
    background-color: rgba(61, 93, 236, 1);
  }
  .copy-btn__coping {
    background-color: rgba(61, 236, 128, 0.8);
  }
  .copy-btn__coping:hover {
    background-color: rgba(61, 236, 128, 0.8);
  }
  .long_t {
    transition: background-color 1s ease;
  }
</style>
