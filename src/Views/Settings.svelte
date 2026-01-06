<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { sep } from "@tauri-apps/api/path";
  import { appConfig, updateConfig, providersWasInited, radioApiProvider, providers } from "../store/main";
  import { choosePath } from "../utils/path";
  import { updateEachVersion, versions } from "../store/upload";

  import Bg from "../Components/Bg.svelte";
  import Radio from "../Components/Radio.svelte";
  import { hasLocalVersion } from "../utils/checks";

  let coping = $state(false);
  let coping2 = $state(false);
  let uuid = $state("");

  async function selectInstallPath(e: Event) {
    await choosePath((selected) => updateConfig("default_installed_path", selected));
    await invoke<void>("set_default_install_path", { path: $appConfig?.default_installed_path });
    const s = await sep();

    updateEachVersion((version) => {
      return {
        ...version,
        installed_path: `${$appConfig?.default_installed_path}${s}${version.path}`,
      };
    });
  }
  async function selectDownloadPath(e: Event) {
    await choosePath((selected) => updateConfig("default_download_path", selected));
    await invoke<void>("set_default_download_path", { path: $appConfig?.default_download_path });
    const s = await sep();

    updateEachVersion((version) => {
      return {
        ...version,
        download_path: `${$appConfig?.default_download_path}${s}${version.path}`,
      };
    });
  }
  async function copyUuid() {
    await writeText(uuid);

    coping = true;
    setTimeout(() => (coping2 = true), 500);
    setTimeout(() => (coping = false), 1000);
    setTimeout(() => (coping2 = false), 1500);
  }

  $effect(() => {
    if ($providersWasInited) {
      invoke<AppConfig>("get_config").then((config) => {
        uuid = config.client_uuid;
      });
    }
  });
  $effect(() => {
    invoke("set_current_api_provider", { provider: $radioApiProvider });

    setTimeout(() => {
      invoke<Version[]>("get_available_versions").then((data) => {
        versions.clear();

        for (const item of data) {
          const found = $versions.find((v) => v.name === item.name);
          const hasLocal = hasLocalVersion(item);
          if (!found && !hasLocal) {
            $versions.push(item);
          }
        }
      });
    }, 500);
  });
</script>

<div class="settings_view">
  <h2>{$_("app.labels.settings")}</h2>

  <div class="input-group">
    <span>{$_("app.settings.clientUuid")}</span>
    <div class="input-row">
      <input type="text" readonly bind:value={uuid} placeholder="" class="uuid-input" />
      <button type="button" onclick={copyUuid} class="copy-btn" class:copy-btn__coping={coping} class:long_t={coping2}>
        {#if coping}
          {$_("app.copy.1")}
        {:else}
          {$_("app.copy.2")}
        {/if}
      </button>
    </div>
  </div>

  <div class="input-group">
    <span>{$_("app.download.defaultInstallPath")}</span>
    <div class="input-row">
      <input type="text" readonly bind:value={$appConfig.default_installed_path} placeholder={$_("app.download.installPath")} class="uuid-input" />
      <button type="button" onclick={selectInstallPath} class="copy-btn">
        {$_("app.releases.browse")}
      </button>
    </div>
  </div>
  <div class="input-group">
    <span>{$_("app.download.defaultDownloadDataPath")}</span>
    <div class="input-row">
      <input
        type="text"
        readonly
        bind:value={$appConfig.default_download_path}
        placeholder={$_("app.download.downloadDataPath")}
        class="uuid-input" />
      <button type="button" onclick={selectDownloadPath} class="copy-btn">
        {$_("app.releases.browse")}
      </button>
    </div>
  </div>

  <div class="input-group">
    <Bg>
      <div class="input-row input-column">
        <span>{$_("app.settings.servers")}</span>
        {#each $providers as [id, stats]}
          <Radio name="provider" value={id} disabled={!stats.available} bind:group={$radioApiProvider}>
            {$_(`app.servers.${id}`)}
            {#if stats.available}
              ({$_("app.settings.ping")} {stats.latency_ms})
            {:else}
              <span class="warntext">({$_("app.settings.noAvailable")})</span>
            {/if}
          </Radio>
        {/each}
      </div>
    </Bg>
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
  .input-column {
    flex-direction: column;
    align-items: baseline;
  }
  .input-group {
    margin-bottom: 1.25rem;
  }

  .warntext {
    font-size: 0.8rem;
    color: rgba(252, 186, 186, 0.8);
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
