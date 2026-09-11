<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { gameStatus, launchError, localVersions, showDlgLaunchError } from "../store/main";
  import { currentView } from "../store/menu";
  import { mainVersion, selectedVersion } from "../store/upload";
  import { normalizeLaunchError } from "../lib/main";

  // Game liveness is owned by the backend GameTracker ($gameStatus mirrors it);
  // the button only sends run_game and shows errors.
  // All startup state (selected version, main version, upload restore) is
  // filled by bootstrap() in lib/bootstrap.ts.
  const launchApp = async () => {
    if (!$mainVersion && !$selectedVersion) {
      currentView.select("versions");
      return;
    }
    if ($gameStatus.running) return;

    // Only treat the launch as "main version" when the selection IS the main
    // version — otherwise run the explicitly selected installed version.
    const useMain = !!$mainVersion && $selectedVersion === $mainVersion.name;
    const versionName = useMain ? null : $selectedVersion;
    if (!useMain && !versionName) {
      currentView.select("versions");
      return;
    }
    if (!useMain && versionName && !$localVersions.get(versionName)) {
      console.error("Selected version not found in localVersions:", versionName);
      return;
    }

    try {
      await invoke<GameStatus>("run_game", { versionName, useMain });
    } catch (e) {
      launchError.set(normalizeLaunchError(e));
      showDlgLaunchError.set(true);
    }
  };
</script>

<span role="button" tabindex="0" class="launchbtn" class:launchbtn_inactive={$gameStatus.running} onclick={launchApp}>
  {#if !$gameStatus.running}
    {#if $selectedVersion}
      {$_("app.launch.start")} {$selectedVersion}
    {:else}
      {$_("app.launch.download")}
    {/if}
  {:else}
    {$_("app.launch.inGame")}
  {/if}
</span>

<style>
  .launchbtn {
    position: absolute;
    bottom: 50px;
    right: 140px;
    padding: 10px 40px;
    background-color: rgba(61, 93, 236, 0.8);
    transition: background-color 0.15s ease;
    border-radius: 3px;
    -webkit-app-region: no-drag;
  }
  .launchbtn:hover {
    cursor: pointer;
    background-color: rgba(61, 93, 236, 1);
  }
  .launchbtn_inactive {
    cursor: default;
    background-color: rgba(0, 0, 0, 0.8);
  }
  .launchbtn_inactive:hover {
    background-color: rgba(0, 0, 0, 0.8);
  }
</style>
