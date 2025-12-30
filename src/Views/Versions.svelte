<!-- ReleasesView.svelte -->
<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { join } from "@tauri-apps/api/path";
  import { connectStatus, inDownloading, localVersions, providersWasInited, versionsWillBeLoaded } from "../store/main";
  import {
    showUploading,
    versions,
    releaseName,
    totalFiles,
    uploadedFiles,
    updateVersion,
    selectedVersion,
    hasAnyLocalVersion,
    updateEachVersion,
    mainVersion,
  } from "../store/upload";
  import { ConnectStatus, DownloadStatus } from "../consts";
  import { Play, Pause, Stop, Installed } from "../Icons";
  import { choosePath } from "../utils/path";
  import { getInGb, getInMb, parseBytes } from "../utils/dwn";

  import Progress from "../Components/Progress.svelte";
  import Button from "../Components/Button.svelte";

  let expandedIndex = $state<number | null>(null);
  let input1Checks = $state<string | null>(null);
  let input2Checks = $state<string | null>(null);
  let input1Needed = $state<number>(0);
  let input2Needed = $state<number>(0);
  let addVersionName = $state<boolean>(true);

  async function fetchVersionManifest(releaseName: string) {
    console.log("Start fetchVersionManifest, releaseName: ", releaseName);
    const manifest = await invoke<ReleaseManifest>("get_release_manifest", { releaseName });
    console.log("fetchVersionManifest, manifest: ", manifest);

    updateVersion(releaseName, () => ({
      manifest,
    }));
  }

  async function handleContinueDownload(releaseName: string) {
    console.log("Start handleContinueDownload");

    updateVersion(releaseName, () => ({
      inProgress: true,
      isStoped: false,
    }));

    try {
      await invoke<void>("continue_download_version", {
        versionName: releaseName,
      });
    } catch (error) {
      console.log("Error continue download release: ", releaseName, error);
      // if (error.message === "USER_CANCELLED")
    } finally {
      updateVersion(releaseName, () => ({
        inProgress: false,
        isStoped: false,
      }));
    }
  }

  async function cancelDownload(event: Event, releaseName: string) {
    console.log("Start handleCancelDownload");

    await invoke<void>("cancel_download_version", {
      releaseName: releaseName,
    });
  }
  async function handleCancelDownload(event: Event, releaseName: string) {
    await cancelDownload(event, releaseName);
    updateVersion(releaseName, () => ({
      inProgress: false,
      isStoped: false,
    }));
    await invoke<void>("remove_download_version", {
      versionName: releaseName,
    });
    await invoke<void>("clear_progress_version", { versionName: releaseName });
  }
  async function handlePauseDownload(event: Event, releaseName: string) {
    await cancelDownload(event, releaseName);
    updateVersion(releaseName, () => ({
      inProgress: false,
      isStoped: true,
    }));
  }

  async function handleStartDownload(event: Event, releaseName: string) {
    event.stopPropagation();

    input1Checks = null;
    input2Checks = null;

    const version = $versions.find((v) => v.name === releaseName);
    if (!version) {
      return;
    }

    const manifest = version.manifest;
    if (!manifest) {
      return;
    }

    if (version.download_path === version.installed_path) {
      input1Needed = manifest.compressed_size + manifest.total_size;
      input2Needed = input1Needed;
      const isSpaceEnough = await invoke<boolean>("check_available_disk_space", { path: version.download_path, needed: input1Needed });
      if (!isSpaceEnough) {
        input1Checks = "space";
        input2Checks = "space";
      }
    } else {
      const [isSpaceEnough, isSpaceEnough2] = await Promise.all([
        invoke<boolean>("check_available_disk_space", { path: version.installed_path, needed: manifest.total_size }),
        invoke<boolean>("check_available_disk_space", { path: version.download_path, needed: manifest.compressed_size }),
      ]);
      if (!isSpaceEnough) {
        input1Checks = "space";
        input1Needed = manifest.total_size;
      }
      if (!isSpaceEnough2) {
        input2Checks = "space";
        input2Needed = manifest.compressed_size;
      }
    }

    if (input1Checks || input2Checks) {
      return;
    }

    console.log("Start handleStartDownload");

    updateVersion(releaseName, () => ({
      inProgress: true,
      isStoped: false,
    }));

    try {
      await invoke<void>("start_download_version", {
        downloadPath: version.download_path,
        installPath: version.installed_path,
        versionName: version.name,
      });
    } catch (error) {
      console.log("Error download release: ", releaseName, error);
      // if (error.message === "USER_CANCELLED")
    } finally {
      updateVersion(releaseName, () => ({
        inProgress: false,
        isStoped: false,
      }));
    }
  }

  async function onChangeAddNamePath(version: Version) {
    let ipath = version.installed_path;
    let dpath = version.download_path;

    if (addVersionName) {
      if (!ipath.includes(version.path)) ipath = await join(ipath, version.path);
      if (!dpath.includes(`${version.path}_data`)) dpath = await join(dpath, `${version.path}_data`);
    } else {
      ipath = ipath.replace(version.path, "");
      dpath = dpath.replace(`${version.path}_data`, "");
    }

    updateVersion(version.name, () => ({
      installed_path: ipath,
      download_path: dpath,
    }));
  }
  async function chooseInstallPath(event: Event, version: Version) {
    event.stopPropagation();

    await choosePath(async (selected) => {
      let path = selected;

      if (addVersionName) {
        path = await join(path, version.path);
      }

      updateVersion(version.name, () => ({
        installed_path: path,
        download_path: `${path}_data`,
      }));
    });
  }
  async function chooseDownloadDataPath(event: Event, version: Version) {
    event.stopPropagation();

    await choosePath((selected) =>
      updateVersion(version.name, () => ({
        download_path: selected,
      })),
    );
  }
  async function chooseInstalledVersion(event: Event, version: Version) {
    event.stopPropagation();

    await invoke<void>("set_current_game_version", { versionName: version.name });
    mainVersion.set(version);
    selectedVersion.set(version.name);
  }
  async function runVersion(event: Event, version: Version) {
    event.stopPropagation();

    await invoke<number>("run_game", { version });
  }
  async function deleteVersion(event: Event, versionName: string) {
    event.stopPropagation();

    console.log("deleteVersion, versionName: ", versionName);

    await invoke<void>("delete_installed_version", { versionName });
  }

  function getStatusText(status: DownloadStatus) {
    switch (status) {
      case DownloadStatus.Init:
        return $_("app.download.text.init");
      case DownloadStatus.DownloadFiles:
        return $_("app.download.text.files");
      case DownloadStatus.Unpacking:
        return $_("app.download.text.unpack");
      default:
        return `Invalid status: ${status}`;
    }
  }

  function toggleExpand(index: number) {
    expandedIndex = expandedIndex === index ? null : index;

    updateEachVersion((v) => {
      console.log("updateEachVersion, v: ", v);
      return v;
    });
  }

  function fetch(index: number) {
    if (index != null) {
      const releaseName = $versions[index]?.name;
      fetchVersionManifest(releaseName);
    }
  }

  $effect(() => {
    if ($providersWasInited) {
      invoke<AppConfig>("get_config").then((config) => {
        if (!$showUploading && !!config.progress_upload) {
          showUploading.set(true);
          releaseName.set(config.progress_upload.name);
          selectedVersion.set(config.selected_version);

          invoke<RepoSyncState | null>("get_upload_manifest").then((manifest) => {
            if (manifest) {
              totalFiles.set(manifest.total_files_count);
              uploadedFiles.set(manifest.uploaded_files_count);
            }
          });
        }
      });
    }
  });
</script>

<div class="releases-view">
  <h2>{$_("app.labels.installedVersions")}</h2>

  <div class="releases-scroll">
    {#if !$hasAnyLocalVersion}
      <span class="version-name">
        {$_("app.releases.noAnyInstalledVersion")}
      </span>
    {/if}
    {#each $localVersions as [name, version], i}
      <div class="release-item">
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div class="header local-versions" role="button" tabindex="0" onclick={() => toggleExpand(i)}>
          <span class="plus-icon">
            {#if name === $selectedVersion}
              <Installed size={28} isButton={false} />
            {/if}
          </span>
          <span class="version-name">
            {name}
          </span>
          <button type="button" onclick={(e) => chooseInstalledVersion(e, version)} class="choose-btn" style="margin-left: auto">
            {#if name === $selectedVersion}
              {$_("app.releases.selected")}
            {:else}
              {$_("app.releases.toSelect")}
            {/if}
          </button>
          <button type="button" onclick={(e) => runVersion(e, version)} class="choose-btn" style="margin-left: auto; margin-right: 10px">
            {$_("app.releases.runVersion")}
          </button>
        </div>
        {#if expandedIndex === i}
          <div class="expanded-content">
            <div class="content-row input-group">
              <button
                type="button"
                onclick={(e) => deleteVersion(e, name)}
                class="choose-btn cancel-btn"
                style="margin-left: auto; margin-right: 10px">
                {$_("app.releases.delete")}
              </button>
            </div>
          </div>
        {/if}
      </div>
    {/each}
  </div>

  <h2>{$_("app.labels.allVersions")}</h2>

  <div class="releases-scroll">
    <!-- Список существующих релизов -->
    {#if !$versionsWillBeLoaded}
      {#if $connectStatus === ConnectStatus.ConnnectError}
        <h2 style="color: rgba(254, 197, 208, 1)">{$_("app.h.error")}</h2>
      {:else}
        <div class="loader-card">
          <svg width="100" height="100" viewBox="0 0 48 48" xmlns="http://www.w3.org/2000/svg">
            <circle cx="12" cy="24" r="4" fill="white" opacity="0.3">
              <animate attributeName="opacity" values="0.3;1;0.3" dur="1.2s" repeatCount="indefinite" />
            </circle>
            <circle cx="24" cy="24" r="4" fill="white" opacity="0.3">
              <animate attributeName="opacity" values="0.3;1;0.3" dur="1.2s" begin="0.2s" repeatCount="indefinite" />
            </circle>
            <circle cx="36" cy="24" r="4" fill="white" opacity="0.3">
              <animate attributeName="opacity" values="0.3;1;0.3" dur="1.2s" begin="0.4s" repeatCount="indefinite" />
            </circle>
          </svg>
          <h2>{$_("app.download.loadData")}</h2>
        </div>
      {/if}
    {:else}
      {#each $versions as version, i}
        {#if !$localVersions.has(version.name)}
          <div class="release-item">
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class="header"
              role="button"
              tabindex="0"
              onclick={() => {
                fetch(i);
                toggleExpand(i + $localVersions.size);
              }}>
              <span class="plus-icon">
                {#if $inDownloading}
                  <svg class="spinner" fill="#FFF" width="24px" height="24px" viewBox="0 0 1000 1000" xmlns="http://www.w3.org/2000/svg"
                    ><path
                      class="fil0"
                      d="M854.569 841.338c-188.268 189.444 -519.825 171.223 -704.157 -13.109 -190.56 -190.56 -200.048 -493.728 -28.483 -695.516 10.739 -12.623 21.132 -25.234 34.585 -33.667 36.553 -22.89 85.347 -18.445 117.138 13.347 30.228 30.228 35.737 75.83 16.531 111.665 -4.893 9.117 -9.221 14.693 -16.299 22.289 -140.375 150.709 -144.886 378.867 -7.747 516.005 152.583 152.584 406.604 120.623 541.406 -34.133 106.781 -122.634 142.717 -297.392 77.857 -451.04 -83.615 -198.07 -305.207 -291.19 -510.476 -222.476l-.226 -.226c235.803 -82.501 492.218 23.489 588.42 251.384 70.374 166.699 36.667 355.204 -71.697 493.53 -11.48 14.653 -23.724 28.744 -36.852 41.948z" />
                  </svg>
                {:else}
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
                {/if}
              </span>
              <span class="version-name">
                {#if $inDownloading}
                  {$_("app.download.inProgress")} {version.name}
                {:else}
                  {version.name}
                {/if}
              </span>
              {#if version.isStoped}
                <Button style="padding: 6px 20px; margin-left: auto" isYellow onclick={() => handleContinueDownload(version.name)}
                  >{$_("app.download.continue")}</Button>
              {/if}
            </div>
            {#if expandedIndex === i + $localVersions.size}
              <div class="expanded-content">
                <div class="content-row input-group">
                  <span class="version-name">
                    {$_("app.download.compressedSize")}
                  </span>
                  <span class="version-name version-size">
                    {#if version.manifest?.compressed_size && version.manifest?.compressed_size > 0}
                      {parseBytes(version.manifest?.compressed_size)[0]}{$_(`app.common.${parseBytes(version.manifest?.compressed_size)[1]}`)}
                    {:else}
                      <svg class="spinner" fill="#FFF" width="24px" height="24px" viewBox="0 0 1000 1000" xmlns="http://www.w3.org/2000/svg"
                        ><path
                          class="fil0"
                          d="M854.569 841.338c-188.268 189.444 -519.825 171.223 -704.157 -13.109 -190.56 -190.56 -200.048 -493.728 -28.483 -695.516 10.739 -12.623 21.132 -25.234 34.585 -33.667 36.553 -22.89 85.347 -18.445 117.138 13.347 30.228 30.228 35.737 75.83 16.531 111.665 -4.893 9.117 -9.221 14.693 -16.299 22.289 -140.375 150.709 -144.886 378.867 -7.747 516.005 152.583 152.584 406.604 120.623 541.406 -34.133 106.781 -122.634 142.717 -297.392 77.857 -451.04 -83.615 -198.07 -305.207 -291.19 -510.476 -222.476l-.226 -.226c235.803 -82.501 492.218 23.489 588.42 251.384 70.374 166.699 36.667 355.204 -71.697 493.53 -11.48 14.653 -23.724 28.744 -36.852 41.948z" />
                      </svg>
                    {/if}
                  </span>
                  <span style="margin-left: 20px"> </span>
                  <span class="version-name">
                    {$_("app.download.totalSize")}
                  </span>
                  <span class="version-name version-size">
                    {#if version.manifest?.total_size}
                      {parseBytes(version.manifest?.total_size)[0]}{$_(`app.common.${parseBytes(version.manifest?.total_size)[1]}`)}
                    {:else}
                      <svg class="spinner" fill="#FFF" width="24px" height="24px" viewBox="0 0 1000 1000" xmlns="http://www.w3.org/2000/svg"
                        ><path
                          class="fil0"
                          d="M854.569 841.338c-188.268 189.444 -519.825 171.223 -704.157 -13.109 -190.56 -190.56 -200.048 -493.728 -28.483 -695.516 10.739 -12.623 21.132 -25.234 34.585 -33.667 36.553 -22.89 85.347 -18.445 117.138 13.347 30.228 30.228 35.737 75.83 16.531 111.665 -4.893 9.117 -9.221 14.693 -16.299 22.289 -140.375 150.709 -144.886 378.867 -7.747 516.005 152.583 152.584 406.604 120.623 541.406 -34.133 106.781 -122.634 142.717 -297.392 77.857 -451.04 -83.615 -198.07 -305.207 -291.19 -510.476 -222.476l-.226 -.226c235.803 -82.501 492.218 23.489 588.42 251.384 70.374 166.699 36.667 355.204 -71.697 493.53 -11.48 14.653 -23.724 28.744 -36.852 41.948z" />
                      </svg>
                    {/if}
                  </span>
                </div>
                {#if !version.inProgress && !version.isStoped}
                  <div class="input-group">
                    <label class="checkbox-label">
                      <input type="checkbox" bind:checked={addVersionName} onchange={(e) => onChangeAddNamePath(version)} />
                      {$_("app.download.addVersionName")}
                    </label>
                  </div>
                  <div class="input-group">
                    <!-- svelte-ignore a11y_label_has_associated_control -->
                    <label class="input-label">{$_("app.download.installPath")}</label>
                    <div class="input-row">
                      <input
                        type="text"
                        readonly
                        bind:value={version.installed_path}
                        placeholder={$_("app.download.installPath")}
                        class="path-input" />
                      <button type="button" onclick={(e) => chooseInstallPath(e, version)} class="choose-btn">
                        {$_("app.releases.browse")}
                      </button>
                    </div>
                    {#if input1Checks}
                      <label class="input-label-2">{$_(`app.input.checks.${input1Checks}`)} {getInGb(input1Needed)}{$_("app.common.sfx")}</label>
                    {/if}
                  </div>
                  <div class="input-group">
                    <!-- svelte-ignore a11y_label_has_associated_control -->
                    <label class="input-label">{$_("app.download.downloadDataPath")}</label>
                    <div class="input-row">
                      <input
                        type="text"
                        readonly
                        bind:value={version.download_path}
                        placeholder={$_("app.download.downloadDataPath")}
                        class="path-input" />
                      <button type="button" onclick={(e) => chooseDownloadDataPath(e, version)} class="choose-btn">
                        {$_("app.releases.browse")}
                      </button>
                    </div>
                    {#if input2Checks}
                      <label class="input-label-2">{$_(`app.input.checks.${input2Checks}`)} {getInGb(input2Needed)}{$_("app.common.sfx")}</label>
                    {/if}
                  </div>
                {/if}
                {#if !version.inProgress && !version.isStoped}
                  <div style="margin-bottom: 50px;"></div>
                {:else}
                  <div class="content-row input-group">
                    <span>
                      {getStatusText(version.status as DownloadStatus)}
                      {$_("app.download.status.progress")}
                      {version.downloadProgress.toFixed(2)}% {$_("app.download.status.files")}
                      {version.downloadedFilesCnt}/{version.totalFileCount}
                      {$_("app.download.status.file")}
                      {version.downloadCurrentFile}
                      {getInMb(version.downloadedFileBytes)}Mb

                      {$_("app.download.status.speed")}
                      {version.speedValue}{version.sfxValue}
                    </span>
                  </div>
                {/if}
                <div class="content-row input-group">
                  {#if version.inProgress || version.isStoped}
                    <Progress progress={version.downloadProgress} />
                  {/if}
                  {#if version.isStoped}
                    <button type="button" onclick={(e) => handleContinueDownload(version.name)} class="download-btn icon-btn continue-btn">
                      <Play size={12} />
                    </button>
                  {:else if version.inProgress}
                    <button type="button" onclick={(e) => handlePauseDownload(e, version.name)} class="download-btn icon-btn continue-btn">
                      <Pause size={12} />
                    </button>
                    <button type="button" onclick={(e) => handleCancelDownload(e, version.name)} class="download-btn icon-btn cancel-btn">
                      <Stop size={12} />
                    </button>
                  {/if}
                  {#if !version.isStoped && !version.inProgress}
                    {#if version.manifest}
                      <button type="button" onclick={(e) => handleStartDownload(e, version.name)} class="download-btn">
                        {$_("app.download.start")}
                      </button>
                    {:else}
                      <button type="button" class="download-btn in-process">
                        {$_("app.download.wait")}
                        <svg class="spinner" fill="#FFF" width="24px" height="24px" viewBox="0 0 1000 1000" xmlns="http://www.w3.org/2000/svg"
                          ><path
                            class="fil0"
                            d="M854.569 841.338c-188.268 189.444 -519.825 171.223 -704.157 -13.109 -190.56 -190.56 -200.048 -493.728 -28.483 -695.516 10.739 -12.623 21.132 -25.234 34.585 -33.667 36.553 -22.89 85.347 -18.445 117.138 13.347 30.228 30.228 35.737 75.83 16.531 111.665 -4.893 9.117 -9.221 14.693 -16.299 22.289 -140.375 150.709 -144.886 378.867 -7.747 516.005 152.583 152.584 406.604 120.623 541.406 -34.133 106.781 -122.634 142.717 -297.392 77.857 -451.04 -83.615 -198.07 -305.207 -291.19 -510.476 -222.476l-.226 -.226c235.803 -82.501 492.218 23.489 588.42 251.384 70.374 166.699 36.667 355.204 -71.697 493.53 -11.48 14.653 -23.724 28.744 -36.852 41.948z" />
                        </svg>
                      </button>
                    {/if}
                  {/if}
                </div>
              </div>
            {/if}
          </div>
        {/if}
      {/each}
    {/if}
  </div>
</div>

<style>
  h2 {
    margin-bottom: 2rem;
  }

  .releases-view {
    display: flex;
    flex-direction: column;
    padding: 1.5rem;
    margin: 0 auto;
    font-family: system-ui, sans-serif;
    overflow: auto;
    height: 86vh;
    -webkit-app-region: no-drag;
  }
  .releases-view::-webkit-scrollbar {
    width: 12px;
  }
  .releases-view::-webkit-scrollbar-track {
    background: transparent;
  }
  .releases-view::-webkit-scrollbar-thumb {
    background-color: rgba(61, 93, 236, 0.8);
    border-radius: 6px;
    border: 3px solid transparent;
    background-clip: content-box;
  }
  .releases-view::-webkit-scrollbar-thumb:hover {
    background-color: rgba(61, 93, 236, 1);
  }
  .releases-view::-webkit-scrollbar-button {
    display: none;
  }

  .releases-scroll {
    -webkit-app-region: no-drag;
    padding-right: 20px;
  }
  .releases-scroll::-webkit-scrollbar {
    width: 12px;
  }
  .releases-scroll::-webkit-scrollbar-track {
    background: transparent;
  }
  .releases-scroll::-webkit-scrollbar-thumb {
    background-color: rgba(61, 93, 236, 0.8);
    border-radius: 6px;
    border: 3px solid transparent;
    background-clip: content-box;
  }
  .releases-scroll::-webkit-scrollbar-thumb:hover {
    background-color: rgba(61, 93, 236, 1);
  }
  .releases-scroll::-webkit-scrollbar-button {
    display: none;
  }

  .release-item {
    -webkit-app-region: no-drag;
    background-color: rgba(40, 40, 40, 0.6);
    border-radius: 6px;
    margin-bottom: 1rem;
    overflow: hidden;
    cursor: pointer;
    transition: background-color 0.2s ease;
  }
  .release-item:hover {
    background-color: rgba(50, 50, 50, 0.7);
  }
  .content-row {
    display: flex;
  }

  .spinner {
    width: 16px;
    height: 16px;
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    0% {
      transform: rotate(0deg);
    }
    100% {
      transform: rotate(360deg);
    }
  }

  .header {
    display: flex;
    align-items: center;
    padding: 1rem 1.25rem;
    gap: 0.75rem;
  }
  .local-versions {
    display: grid;
    grid-template-columns: 40px 1fr 100px 100px;
    justify-items: baseline;
  }

  .plus-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    font-size: 1.25rem;
    color: #4caf50;
    font-weight: bold;
  }

  .version-name {
    color: white;
    font-weight: 500;
    margin-right: 10px;
  }

  .input-group {
    margin-bottom: 1.25rem;
  }
  .input-label {
    display: block;
    margin-bottom: 0.5rem;
    color: #fff;
    font-weight: 500;
  }
  .input-label-2 {
    display: block;
    margin-bottom: 0.5rem;
    color: #f55858;
    font-size: small;
  }
  .input-row {
    display: flex;
    gap: 0.75rem;
  }

  .path-input {
    -webkit-app-region: no-drag;
    flex: 1;
    padding: 0.5rem;
    border: 1px solid #555;
    border-radius: 4px;
    background-color: rgba(255, 255, 255, 0.8);
    width: 95%;
  }
  .path-input:focus {
    background-color: rgba(255, 255, 255, 1);
    outline: none;
  }

  .choose-btn {
    -webkit-app-region: no-drag;
    padding: 0.5rem 1rem;
    color: #fff;
    background-color: rgba(61, 93, 236, 0.8);
    border: none;
    border-radius: 3px;
    cursor: pointer;
    transition: background-color 0.15s ease;
  }
  .choose-btn:hover {
    background-color: rgba(61, 93, 236, 1);
  }

  .expanded-content {
    padding: 1rem 1.25rem 1.25rem;
    border-top: 1px solid rgba(255, 255, 255, 0.1);
    overflow-y: auto;
    max-height: 500px;
  }
  .expanded-content::-webkit-scrollbar {
    width: 12px;
  }
  .expanded-content::-webkit-scrollbar-track {
    background: transparent;
  }
  .expanded-content::-webkit-scrollbar-thumb {
    background-color: rgba(61, 93, 236, 0.8);
    border-radius: 6px;
    border: 3px solid transparent;
    background-clip: content-box;
  }
  .expanded-content::-webkit-scrollbar-thumb:hover {
    background-color: rgba(61, 93, 236, 1);
  }
  .expanded-content::-webkit-scrollbar-button {
    display: none;
  }

  .download-btn {
    -webkit-app-region: no-drag;
    padding: 0.6rem 1.5rem;
    color: white;
    background-color: rgba(76, 175, 80, 0.8);
    border: none;
    border-radius: 4px;
    cursor: pointer;
    font-weight: 500;
    transition: background-color 0.15s ease;
    margin-left: auto;
  }
  .download-btn:hover {
    background-color: rgba(76, 175, 80, 1);
  }
  .download-btn.in-process {
    background-color: rgba(100, 100, 100, 1);
  }
  .download-btn.in-process:hover {
    background-color: rgba(100, 100, 100, 1);
  }

  .icon-btn {
    padding: 0.5rem 0.8rem;
    margin-left: 1rem;
    align-self: center;
  }
  .icon-btn:hover {
  }

  .cancel-btn {
    background-color: rgba(251, 50, 0, 0.8);
  }
  .cancel-btn:hover {
    background-color: rgba(251, 50, 0, 1);
  }

  .continue-btn {
    background-color: rgba(236, 180, 61, 0.8);
  }
  .continue-btn:hover {
    background-color: rgba(236, 180, 61, 1);
  }

  .checkbox-label {
    -webkit-app-region: no-drag;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.75rem;
    width: 60%;
  }
  .checkbox-label:hover {
    cursor: pointer;
  }

  /* Скрыть стандартный чекбокс */
  .checkbox-label input[type="checkbox"] {
    -webkit-app-region: no-drag;
    appearance: none;
    width: 20px;
    height: 20px;
    border: 2px solid white;
    border-radius: 50%;
    background: rgba(30, 30, 30, 0.8);
    outline: none;
    cursor: pointer;
    position: relative;
    transition: background 0.2s ease;
  }

  /* Синий кружок внутри */
  .checkbox-label input[type="checkbox"]::after {
    content: "";
    position: absolute;
    top: 50%;
    left: 50%;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #007acc; /* Синий цвет */
    opacity: 0;
    transform: translate(-50%, -50%) scale(0.8);
    transition:
      opacity 0.25s ease,
      transform 0.25s ease;
  }

  /* Показываем кружок, когда чекбокс checked */
  .checkbox-label input[type="checkbox"]:checked::after {
    opacity: 1;
    transform: translate(-50%, -50%) scale(1);
  }

  /* Опционально: hover-эффект */
  .checkbox-label input[type="checkbox"]:hover {
    background: rgba(40, 40, 40, 0.7);
  }
</style>
