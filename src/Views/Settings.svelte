<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { sep } from "@tauri-apps/api/path";
  import { appConfig, updateConfig, providersWasInited, radioApiProvider, providers, localVersions } from "../store/main";
  import { choosePath } from "../utils/path";
  import { updateEachVersion, versions } from "../store/upload";

  import Scroll from "../Components/Scroll.svelte";
  import Bg from "../Components/Bg.svelte";
  import Radio from "../Components/Radio.svelte";
  import Spin from "../Components/Spin.svelte";
  import { RefreshCw } from "lucide-svelte";
  import { prepareVersionItem } from "../lib/main";
  import { switchProvider, pingProvider } from "../lib/providers";

  let coping = $state(false);
  let coping2 = $state(false);
  let uuid = $state("");
  let providerSwitchError = $state("");
  let pingingProvider = $state<string | null>(null);

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
        download_path: `${$appConfig?.default_download_path}${s}${version.path}_data`,
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

  function hasLocalVersion(version: Version) {
    for (const [name, local] of $localVersions) {
      if (name === version.name) return true;
      if (local.path === version.name) return true;
      if (local.path === version.path) return true;
    }

    return false;
  }

  $effect(() => {
    if ($providersWasInited) {
      invoke<AppConfig>("get_config").then((config) => {
        uuid = config.client_uuid;
      });
    }
  });

  // React to provider radio changes — centralized switch via lib/providers.ts.
  // Skip the initial run to avoid refetching on Settings mount.
  let providerInitialized = false;
  $effect(() => {
    const provider = $radioApiProvider;
    if (!providerInitialized) {
      providerInitialized = true;
      return;
    }
    providerSwitchError = "";
    switchProvider(provider).catch(() => {
      providerSwitchError = $_("app.settings.providerSwitchError");
    });
  });

  async function handlePingProvider(id: string) {
    if (pingingProvider) return;
    pingingProvider = id;
    try {
      await pingProvider(id);
    } catch (e) {
      console.error("pingProvider failed:", e);
    } finally {
      pingingProvider = null;
    }
  }
</script>

<div class="settings_view">
  <h2>{$_("app.labels.settings")}</h2>

  <Scroll value={240}>
    <Bg>
      <span>{$_("app.settings.clientUuid")}</span>
      <div style="margin-bottom: 10px;" />
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
    </Bg>

    <div style="margin-bottom: 20px;" />

    <Bg>
      <span>{$_("app.download.defaultInstallPath")}</span>
      <div style="margin-bottom: 10px;" />
      <div class="input-row">
        <input type="text" readonly bind:value={$appConfig.default_installed_path} placeholder={$_("app.download.installPath")} class="uuid-input" />
        <button type="button" onclick={selectInstallPath} class="copy-btn">
          {$_("app.releases.browse")}
        </button>
      </div>
    </Bg>

    <div style="margin-bottom: 20px;" />

    <Bg>
      <span>{$_("app.download.defaultDownloadDataPath")}</span>
      <div style="margin-bottom: 10px;" />
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
    </Bg>

    <div style="margin-bottom: 20px;" />

    <Bg>
      <div class="input-row input-column">
        <span>{$_("app.settings.servers")}</span>
        {#each $providers as [id, stats]}
          <div class="provider-row">
            <Radio name="provider" value={id} disabled={!stats.available} bind:group={$radioApiProvider}>
              {$_(`app.servers.${id}`)}
              {#if stats.available}
                ({$_("app.settings.ping")} {stats.latency_ms})
              {:else}
                <span class="warntext">({$_("app.settings.noAvailable")})</span>
              {/if}
            </Radio>
            <button
              type="button"
              class="ping-btn"
              onclick={() => handlePingProvider(id)}
              title={$_("app.settings.retryPing")}
            >
              {#if pingingProvider === id}
                <Spin size={14} color="rgba(200, 200, 200, 0.8)" />
              {:else}
                <RefreshCw size={14} color="rgba(200, 200, 200, 0.8)" />
              {/if}
            </button>
          </div>
        {/each}
        {#if providerSwitchError}
          <span class="warntext" style="margin-top: 4px;">{providerSwitchError}</span>
        {/if}
      </div>
    </Bg>
  </Scroll>
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
  }
  .input-column {
    flex-direction: column;
    align-items: baseline;
  }

  .warntext {
    font-size: 0.8rem;
    color: rgba(252, 186, 186, 0.8);
  }

  .provider-row {
    display: flex;
    align-items: center;
    width: 100%;
  }

  .ping-btn {
    -webkit-app-region: no-drag;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 4px;
    border: none;
    border-radius: 4px;
    background: transparent;
    cursor: pointer;
    opacity: 0.6;
    transition: opacity 0.15s ease;
  }

  .ping-btn:hover {
    opacity: 1;
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
