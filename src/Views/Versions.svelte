<!-- ReleasesView.svelte -->
<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { join } from "@tauri-apps/api/path";
  import {
    connectStatus,
    localVersions,
    versionsWillBeLoaded,
    expandedIndex,
    showDlgRemoveVersion,
    removeVersion,
    removeVersionInProcess,
    moveProgress,
    updateLocalVersion,
    showDlgAddVersion,
    appConfig,
    patchCheckResults,
    patchInstallProgress,
    showDlgPatchNotes,
    patchNotesData,
    fetchLocalVersions,
  } from "../store/main";
  import { versions, updateVersion, selectedVersion, hasAnyLocalVersion, updateEachVersion, mainVersion } from "../store/upload";
  import { COFF_FROM_COMPRESSED_SIZE, ConnectStatus, DownloadStatus } from "../consts";
  import { Play, Pause, Stop, Installed, CinC, Installed2 } from "../Icons";
  import { FileDown } from "lucide-svelte";
  import { choosePath } from "../utils/path";
  import { getInGb, parseBytes } from "../utils/dwn";

  import Progress from "../Components/Progress.svelte";
  import Button from "../Components/Button.svelte";
  import Spin from "../Components/Spin.svelte";
  import { onMount } from "svelte";

  let input1Checks = $state<string | null>(null);
  let input2Checks = $state<string | null>(null);
  let input1Needed = $state<number>(0);
  let input2Needed = $state<number>(0);
  let addVersionName = $state<boolean>(true);

  // Patch state
  let patchChecks = $state<Map<string, PatchCheckResult>>(new Map());
  let checkingPatch = $state<string | null>(null);
  let installingPatch = $state<{ version: string; patch: string } | null>(null);
  let patchErrors = $state<Map<string, string>>(new Map());

  function parseSize(size: number | null): string {
    if (!size) return "";
    const parsed = parseBytes(size);
    return `${parsed[0]} ${$_(`app.common.${parsed[1]}`)}`;
  }

  async function handleCheckPatches(version: Version) {
    const name = version.name;
    checkingPatch = name;
    patchErrors.delete(name);
    patchErrors = patchErrors;

    try {
      const result = await invoke<PatchCheckResult>("get_version_patches", { versionName: name });
      patchChecks = new Map(patchChecks).set(name, result);
      patchCheckResults.setItem(name, { count: result.missing.length, checkedAt: Date.now() });
    } catch (e: any) {
      const msg = typeof e === "string" ? e : String(e?.message ?? e);
      patchErrors = new Map(patchErrors).set(name, msg);
      patchCheckResults.delItem(name);
    } finally {
      checkingPatch = null;
    }
  }

  async function handleInstallPatch(version: Version, patchName: string) {
    installingPatch = { version: version.name, patch: patchName };
    const name = version.name;
    patchErrors.delete(name);
    patchErrors = patchErrors;

    try {
      await invoke<void>("start_install_patch", { versionName: name, patchName });
      // Refresh installed_updates in the local store.
      await fetchLocalVersions();
      // Re-check available patches.
      await handleCheckPatches(version);
    } catch (e: any) {
      const msg = typeof e === "string" ? e : String(e?.message ?? e);
      patchErrors = new Map(patchErrors).set(name, msg);
    } finally {
      installingPatch = null;
    }
  }

  async function handleCancelInstall(versionName: string) {
    await invoke<void>("cancel_install_patch", { versionName });
  }

  function openPatchNotes(title: string, notes: string | null) {
    $patchNotesData = { title, notes };
    $showDlgPatchNotes = true;
  }

  function formatInstalledDate(iso: string | undefined): string {
    if (!iso) return "";
    return iso.slice(0, 10);
  }

  function filesProgressFromManifest(manifest: ReleaseManifest, old?: Map<string, VersionFileDownload>) {
    const map = new Map<string, VersionFileDownload>();
    for (const file of manifest.files) {
      const prev = old?.get(file.name);
      map.set(file.name, {
        downloadProgress: prev && file.size > 0 ? (prev.downloadedFileBytes / file.size) * 100 : 0,
        downloadedFileBytes: prev?.downloadedFileBytes || 0,
        totalFileBytes: file.size,
        unpackProgress: prev?.unpackProgress || 0,
        downloadSpeed: 0,
        speedValue: 0,
        sfxValue: "",
        status: prev?.status || 0,
      });
    }
    return map;
  }

  async function fetchVersionManifest(releaseName: string) {
    const found = $versions.find((v) => v.name === releaseName);
    if (found?.manifest) {
      if (!found.filesProgress || found.filesProgress.size === 0) {
        updateVersion(releaseName, (version) => ({
          filesProgress: filesProgressFromManifest(found.manifest!, version.filesProgress),
        }));
      }
      return;
    }

    const manifest = await invoke<ReleaseManifest>("get_release_manifest", { releaseName });

    updateVersion(releaseName, (version) => ({
      manifest,
      filesProgress: filesProgressFromManifest(manifest, version.filesProgress),
    }));
  }

  async function handleContinueDownload(
    event: MouseEvent & {
      currentTarget: EventTarget & HTMLButtonElement;
    },
    version: Version,
    index: number,
  ) {
    event.preventDefault();
    event.stopPropagation();

    console.log("Start handleContinueDownload");

    if ($expandedIndex !== index) {
      $expandedIndex = index;
    }

    updateVersion(version.name, () => ({
      inProgress: true,
      isStoped: false,
      status: DownloadStatus.DownloadFiles,
    }));

    try {
      await invoke<void>("continue_download_version", {
        versionName: version.name,
      });
    } catch (error: any) {
      const msg = typeof error === "string" ? error : String(error?.message ?? error);
      if (msg.includes("USER_CANCELLED") && !version.wasCanceled) {
        updateVersion(version.name, () => ({
          inProgress: false,
          isStoped: true,
        }));
      } else {
        updateVersion(version.name, () => ({
          inProgress: false,
          isStoped: false,
        }));
      }
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
    // Reset to a fresh state (not just inProgress/isStoped) so no stale progress
    // fields linger on the version in the store after cancel.
    updateVersion(releaseName, () => ({
      inProgress: false,
      isStoped: false,
      wasCanceled: true,
      downloadProgress: 0,
      downloadedFilesCnt: 0,
      totalFileCount: 0,
      downloadSpeed: 0,
      speedValue: 0,
      sfxValue: "",
      downloadCurrentFile: "",
      downloadedFileBytes: 0,
      filesProgress: new Map(),
      status: DownloadStatus.Init,
    }));
    // remove_install_dir reads progress_download, so it must run BEFORE
    // remove_download_version/clear_progress_version (which delete that entry).
    try {
      await invoke<void>("remove_install_dir", { versionName: releaseName });
    } catch (e) {
      console.error("remove_install_dir failed:", e);
    }
    await invoke<void>("remove_download_version", {
      versionName: releaseName,
    });
    await invoke<void>("clear_progress_version", { versionName: releaseName });
    // Refresh the frontend appConfig copy so progress_download is in sync.
    // prepareVersionItem reads $appConfig; without this, a later list refresh
    // would see the stale entry and mark the version as paused (isStoped=true).
    try {
      const cfg = await invoke<AppConfig>("get_config");
      appConfig.set(cfg);
    } catch (e) {
      console.error("appConfig refresh after cancel failed:", e);
    }
  }
  async function handlePauseDownload(event: Event, releaseName: string) {
    await cancelDownload(event, releaseName);
    updateVersion(releaseName, () => ({
      inProgress: false,
      isStoped: true,
    }));
  }
  async function handleAddVersion() {
    showDlgAddVersion.set(true);
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

    // Any install/download path is now allowed (Cyrillic/spacesincl.): the launcher handles non-ASCII via a subst virtual drive, so the old
    // [\sА-Яа-я] path guards are removed.

    if (version.download_path === version.installed_path) {
      input1Needed = manifest.compressed_size + manifest.total_size * COFF_FROM_COMPRESSED_SIZE;
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
        input2Needed = manifest.compressed_size * COFF_FROM_COMPRESSED_SIZE;
      }
    }

    if (input1Checks || input2Checks) {
      return;
    }

    console.log("Start handleStartDownload");

    updateVersion(releaseName, () => ({
      inProgress: true,
      isStoped: false,
      wasCanceled: false,
      filesProgress: filesProgressFromManifest(manifest),
      status: DownloadStatus.DownloadFiles,
    }));

    try {
      await invoke<void>("start_download_version", {
        downloadPath: version.download_path,
        installPath: version.installed_path,
        versionName: version.name,
      });
    } catch (error: any) {
      const updatedVersion = $versions.find((v) => v.name === releaseName);
      const msg = typeof error === "string" ? error : String(error?.message ?? error);
      if (msg.includes("USER_CANCELLED") && !updatedVersion!.wasCanceled) {
        updateVersion(releaseName, () => ({
          inProgress: false,
          isStoped: true,
        }));
      } else {
        updateVersion(releaseName, () => ({
          inProgress: false,
          isStoped: false,
        }));
      }
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
    await choosePath((selected) => {
      updateVersion(version.name, () => ({
        download_path: selected,
      }));
    });
  }
  async function chooseInstalledVersion(event: Event, version: Version) {
    event.stopPropagation();

    await invoke<void>("set_current_game_version", { versionName: version.name });
    mainVersion.set(version);
    selectedVersion.set(version.name);
  }
  async function runVersion(event: Event, version: Version) {
    event.stopPropagation();

    await invoke<number>("run_game", { versionName: version.name, useMain: false });
  }
  async function deleteVersion(event: Event, version: Version) {
    event.stopPropagation();

    $removeVersion = version;
    $showDlgRemoveVersion = true;
  }
  async function handleMoveVerson(version: Version) {
    const selected = await choosePath(() => {});

    if (!selected) {
      return;
    }

    await invoke("move_version", { versionName: version.path, dest: selected });

    updateLocalVersion(version.name, (version) => ({
      ...version,
      installed_path: selected,
    }));

    setTimeout(() => {
      moveProgress.delItem(version.name);
    }, 2000);
  }
  async function handleOpenGameDir(version: Version) {
    await invoke("open_explorer", { path: version.installed_path });
  }
  async function handleOpenLogDir(version: Version) {
    const path = await join(version.installed_path, "appdata", "logs");
    await invoke("open_explorer", { path });
  }

  function getStatusText(status: DownloadStatus) {
    switch (status) {
      case DownloadStatus.Init:
        return $_("app.download.text.init");
      case DownloadStatus.Pause:
        return $_("app.download.text.pause");
      case DownloadStatus.DownloadFiles:
        return $_("app.download.text.files");
      case DownloadStatus.Unpacking:
        return $_("app.download.text.unpack");
      default:
        return `Invalid status: ${status}`;
    }
  }

  function toggleExpand(index: number) {
    $expandedIndex = $expandedIndex === index ? null : index;

    updateEachVersion((v) => {
      return v;
    });
  }

  function fetch(index: number) {
    if (index != null) {
      const releaseName = $versions[index]?.name;
      fetchVersionManifest(releaseName);
    }
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
    $selectedVersion = $selectedVersion;
    $expandedIndex = $expandedIndex;
  });

  onMount(() => {
    // Versions.svelte is destroyed and remounted on every view switch (App.svelte
    // uses a keyed each block). Don't blindly collapse the panel on mount: if a
    // version is actively downloading or paused, re-expand it so the progress UI
    // survives navigation away and back. Otherwise (nothing in progress) keep the
    // default collapsed state.
    const inProgressIdx = $versions.findIndex((v) => v.inProgress || v.isStoped);
    $expandedIndex = inProgressIdx >= 0 ? inProgressIdx + $localVersions.size : null;
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
            {:else}
              <CinC size={28} isButton={false} />
            {/if}
          </span>
          <span class="version-name">
            {name}
            {#if ($patchCheckResults.get(name)?.count ?? 0) > 0}
              <span class="patch-badge">{$patchCheckResults.get(name)!.count}</span>
            {/if}
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
        {#if $expandedIndex === i}
          <div class="expanded-content">
            <div class="content-row input-group">
              <div>
                <span>{$_("app.releases.installedPath")}</span>
                <span>{version.installed_path}</span>
              </div>
              <button
                type="button"
                onclick={(e) => deleteVersion(e, version)}
                class="choose-btn cancel-btn"
                style="margin-left: auto; margin-right: 10px">
                {#if $removeVersionInProcess && $removeVersion?.name === version.name}
                  {$_("app.releases.deleting")}
                  <Spin size={16} />
                {:else}
                  {$_("app.releases.delete")}
                {/if}
              </button>
            </div>
            <div class="input-group">
              <div class="input-buttons">
                <Button size="slim" onclick={() => handleMoveVerson(version)}>
                  {$_("app.releases.move")}
                </Button>
                <Button size="slim" onclick={() => handleOpenGameDir(version)}>
                  {$_("app.releases.openDir")}
                </Button>
                <Button size="slim" onclick={() => handleOpenLogDir(version)}>
                  {$_("app.releases.openLogDir")}
                </Button>
              </div>
            </div>

            <!-- Patch section -->
            <div class="patch-section">
              <div class="patch-section-header">
                <span class="patch-section-title">{$_("app.patches.title")}</span>
                <button
                  type="button"
                  class="choose-btn patch-check-btn"
                  disabled={checkingPatch === name}
                  onclick={() => handleCheckPatches(version)}>
                  {#if checkingPatch === name}
                    <Spin size={12} /> {$_("app.patches.checking")}
                  {:else}
                    {$_("app.patches.check")}
                  {/if}
                </button>
              </div>

              <!-- Installed patches -->
              {#if version.installed_updates.length > 0}
                <div class="patch-subsection">{$_("app.patches.installed")}</div>
                {#each version.installed_updates as patch}
                  <div class="patch-row">
                    <span
                      class="patch-name clickable"
                      onclick={() => openPatchNotes(patch.name, patch.notes ?? null)}>
                      {patch.name}
                    </span>
                    <span class="patch-date">{formatInstalledDate(patch.installed_at)}</span>
                  </div>
                {/each}
              {/if}

              <!-- Available patches (from check result) -->
              {#if patchChecks.has(name)}
                {@const check = patchChecks.get(name)!}
                {#if check.missing.length > 0}
                  <div class="patch-subsection">{$_("app.patches.available")}</div>
                  {#each check.patches.filter((p) => check.missing.includes(p.name)) as patch}
                    <div class="patch-row" class:patch-next={patch.is_next}>
                      <span
                        class="patch-name clickable"
                        onclick={() => openPatchNotes(patch.name, patch.notes)}>
                        {patch.name}
                      </span>
                      <span class="patch-size">{parseSize(patch.size)}</span>
                      {#if patch.is_next}
                        <button
                          type="button"
                          class="download-btn patch-install-btn"
                          class:patch-install-btn-busy={installingPatch?.version === name}
                          disabled={installingPatch?.version === name}
                          onclick={() => handleInstallPatch(version, patch.name)}>
                          {#if installingPatch?.version === name}
                            <Spin size={12} /> {$_("app.patches.installing")}
                          {:else}
                            {$_("app.patches.install")}
                          {/if}
                        </button>
                      {:else}
                        <span class="patch-hint">{$_("app.patches.installNextFirst")}</span>
                      {/if}
                    </div>
                  {/each}
                {:else}
                  <div class="patch-up-to-date">{$_("app.patches.upToDate")}</div>
                {/if}
              {/if}

              <!-- Install progress -->
              {#if installingPatch?.version === name && $patchInstallProgress}
                <div class="patch-install-progress">
                  <div class="patch-install-stage">
                    {#if $patchInstallProgress.stage === "download"}
                      {$_("app.patches.stageDownload")}
                    {:else if $patchInstallProgress.stage === "unpack"}
                      {$_("app.patches.stageUnpack")}
                    {:else if $patchInstallProgress.stage === "delete"}
                      {$_("app.patches.stageDelete")}
                    {:else}
                      {$patchInstallProgress.stage}
                    {/if}
                    {#if $patchInstallProgress.file}
                      — {$patchInstallProgress.file}
                    {/if}
                  </div>
                  <Progress progress={$patchInstallProgress.total_progress} />
                  {#if $patchInstallProgress.stage === "download"}
                  <button
                    type="button"
                    class="cancel-btn choose-btn patch-cancel-btn"
                    onclick={() => handleCancelInstall(name)}>
                    {$_("app.patches.cancel")}
                  </button>
                  {/if}
                </div>
              {/if}

              <!-- Error -->
              {#if patchErrors.has(name)}
                <div class="patch-error">{patchErrors.get(name)}</div>
              {/if}
            </div>

            {#if $moveProgress.has(version.name)}
              <div class="input-group">
                <div class="input-buttons">
                  <span>{$_("app.releases.moving")}</span>
                  {#if $moveProgress.get(version.name)!.percentage !== 100}
                    <span>{$_("app.releases.movingFileName")}</span>
                    <span>{$moveProgress.get(version.name)!.file_name}</span>
                    <span
                      >{parseBytes($moveProgress.get(version.name)!.bytes_moved)[0]}{$_(
                        `app.common.${parseBytes($moveProgress.get(version.name)!.bytes_moved)[1]}`,
                      )} / {parseBytes($moveProgress.get(version.name)!.total_bytes)[0]}{$_(
                        `app.common.${parseBytes($moveProgress.get(version.name)!.total_bytes)[1]}`,
                      )}</span>
                  {:else}
                    <span>{$_("app.releases.movingCompleted")}</span>
                  {/if}
                </div>
                <Progress progress={$moveProgress.get(version.name)!.percentage} />
              </div>
            {/if}
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
        {#if !hasLocalVersion(version)}
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
                {#if version.inProgress}
                  <Spin size={16} />
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
                {#if version.inProgress}
                  {$_("app.download.inProgress")} {version.name}
                {:else}
                  {version.name}
                {/if}
              </span>
              {#if version.isStoped}
                <Button
                  style="margin-left: auto;"
                  size="slim"
                  isYellow
                  onclick={(e: any) => handleContinueDownload(e, version, i + $localVersions.size)}>{$_("app.download.continue")}</Button>
              {/if}
            </div>
            {#if $expandedIndex === i + $localVersions.size}
              <div class="expanded-content">
                {#if version.status !== DownloadStatus.Unpacking}
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
                {/if}
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
                      {getStatusText(version.status as DownloadStatus)} -
                      {#if version.status === DownloadStatus.Unpacking}
                        {version.downloadProgress.toFixed(2)}%
                      {:else}
                        {$_("app.download.status.progress")}
                        {version.downloadProgress.toFixed(2)}% -
                        {$_("app.download.status.files")}
                        {version.downloadedFilesCnt}/{version.totalFileCount} -

                        {$_("app.download.status.speed")}
                        {version.speedValue}
                        {version.sfxValue}
                      {/if}
                    </span>
                  </div>
                {/if}
                <div class="content-row input-group">
                  {#if version.inProgress || version.isStoped}
                    <Progress progress={version.downloadProgress} />
                  {/if}
                  {#if version.isStoped}
                    <button
                      type="button"
                      onclick={(e) => handleContinueDownload(e, version, i + $localVersions.size)}
                      class="download-btn icon-btn continue-btn">
                      <Play size={12} />
                    </button>
                  {:else if version.inProgress && version.status === DownloadStatus.DownloadFiles}
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
                {#if !version.inProgress && !version.isStoped}
                  <div style="margin-bottom: 2px;"></div>
                {:else}
                  <div class="content-row">
                    <span>{$_("app.download.filesStats")}</span>
                  </div>
                  {#if version.inProgress || version.isStoped}
                    {#each version.filesProgress as [name, progress], i}
                      <div class="file-row">
                        <span style="justify-self: end; align-content: center;">
                          {#if progress.status === 0}
                            <FileDown size={12} />
                          {:else if progress.status === 1}
                            <Spin size={12} />
                          {:else if progress.status === 2}
                            <Spin size={12} />
                          {:else if progress.status === 3}
                            <Installed2 size={16} isButton={false} />
                          {/if}
                        </span>

                        <span>{name}</span>

                        <div class="one-column">
                          <Progress height={12} maxWidth="1fr - 300px" progress={progress.downloadProgress} showPercents={false} />
                          {#if progress.downloadProgress >= 100}
                            <Progress
                              height={4}
                              maxWidth="1fr - 300px"
                              style="margin-top: 0;"
                              progress={progress.unpackProgress}
                              showPercents={false} />
                          {/if}
                        </div>

                        <span style="justify-self: end;"
                          >{parseBytes(progress.downloadedFileBytes)[0]}
                          {$_(`app.common.${parseBytes(progress.downloadedFileBytes)[1]}`)} / {parseBytes(progress.totalFileBytes)[0]}
                          {$_(`app.common.${parseBytes(progress.totalFileBytes)[1]}`)}</span>

                        <span style="justify-self: end;">{progress.speedValue} {progress.sfxValue}</span>
                      </div>
                    {/each}
                  {/if}
                {/if}
              </div>
            {/if}
          </div>
        {/if}
      {/each}
    {/if}
  </div>

  <div class="btn-bar">
    <Button onclick={handleAddVersion}>{$_("app.btn.addVersion")}</Button>
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
    height: 77vh;
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

  .btn-bar {
    display: flex;
    position: absolute;
    bottom: 50px;
    right: 40px;
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
  .file-row {
    display: grid;
    grid-template-columns: 20px 100px 1fr 160px 100px;
  }

  .one-column {
    display: flex;
    flex-direction: column;
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

  .input-buttons {
    display: flex;
    gap: 10px;
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
    max-height: 800px;
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

  /* Patch section */
  .patch-section {
    border-top: 1px solid rgba(255, 255, 255, 0.08);
    margin-top: 0.75rem;
    padding-top: 0.75rem;
  }
  .patch-section-header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-bottom: 0.75rem;
  }
  .patch-section-title {
    color: #fff;
    font-weight: 600;
    font-size: 0.95rem;
  }
  .patch-check-btn {
    font-size: 0.8rem;
    padding: 0.3rem 0.8rem;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .patch-check-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  .patch-subsection {
    color: #aaa;
    font-size: 0.8rem;
    font-weight: 500;
    margin-bottom: 0.35rem;
    margin-top: 0.5rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .patch-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.3rem 0;
    font-size: 0.85rem;
  }
  .patch-row.patch-next {
    background: rgba(76, 175, 80, 0.08);
    border-radius: 4px;
    padding: 0.35rem 0.5rem;
  }
  .patch-name {
    color: #ddd;
    min-width: 120px;
  }
  .patch-name.clickable {
    color: #6db3f2;
    cursor: pointer;
    text-decoration: underline;
    text-decoration-style: dotted;
  }
  .patch-name.clickable:hover {
    color: #90c8ff;
  }
  .patch-date {
    color: #888;
    font-size: 0.8rem;
  }
  .patch-size {
    color: #999;
    font-size: 0.8rem;
  }
  .patch-install-btn {
    font-size: 0.8rem;
    padding: 0.3rem 1rem;
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .patch-install-btn-busy {
    background-color: rgba(233, 236, 61, 0.8) !important;
    cursor: wait !important;
  }
  .patch-install-btn-busy:hover {
    background-color: rgba(233, 236, 61, 0.8) !important;
  }
  .patch-hint {
    color: #777;
    font-size: 0.75rem;
    font-style: italic;
    margin-left: auto;
  }
  .patch-up-to-date {
    color: #4caf50;
    font-size: 0.85rem;
    margin-top: 0.5rem;
  }
  .patch-install-progress {
    margin-top: 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .patch-install-stage {
    color: #ddd;
    font-size: 0.85rem;
  }
  .patch-cancel-btn {
    align-self: flex-start;
    font-size: 0.8rem;
    padding: 0.3rem 1rem;
    margin-top: 0.25rem;
  }
  .patch-error {
    color: #f44336;
    font-size: 0.85rem;
    margin-top: 0.5rem;
  }
  .patch-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background-color: rgba(255, 193, 7, 0.9);
    color: #000;
    font-size: 0.7rem;
    font-weight: 700;
    min-width: 18px;
    height: 18px;
    border-radius: 9px;
    padding: 0 5px;
    margin-left: 8px;
    vertical-align: middle;
  }
</style>
