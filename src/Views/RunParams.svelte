<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { appConfig, configReady, startupState, showDlgMaxPerformancePresetWarning } from "../store/main";
  import { ALIFE_DEFAULTS, ALIFE_RANGES, LangType, RenderType, ScopeType } from "../consts";
  import { BiMap } from "../utils/BiMap";

  import Scroll from "../Components/Scroll.svelte";
  import TrackBar from "../Components/TrackBar.svelte";
  import Bg from "../Components/Bg.svelte";

  const langMap = new BiMap<LangType, string>([
    [LangType.Rus, "Русский"],
    [LangType.Eng, "English"],
  ]);
  const renderersMap = new BiMap<RenderType, string>([
    [RenderType.RendererR2, "renderer_r2"],
    [RenderType.RendererR25, "renderer_r2_5"],
    [RenderType.RendererR3, "renderer_r3"],
    [RenderType.RendererR4, "renderer_r4"],
    [RenderType.RendererRgl, "renderer_rgl"],
  ]);
  const scopesMap = new BiMap<ScopeType, string>([
    [ScopeType.Scopes2dStatic, "g_3d_scopes_0"],
    [ScopeType.Scopes3d, "g_3d_scopes_1"],
    [ScopeType.Scopes2dRenderTarget, "g_3d_scopes_2"],
  ]);

  const MAX_PERFORMANCE_PRESET_ID = "max_performance";

  let saving = $state(false);
  let saving2 = $state(false);
  /// Result of the last save: empty while everything was written, otherwise a
  /// localized line saying what the launcher could NOT apply. The command used
  /// to answer plain success even when nothing had been patched.
  let applyStatus = $state("");

  /// Mirrors handlers::user_ltx::ApplyOutcome (serde camelCase).
  type ApplyOutcome = "applied" | "skippedNoVersion" | "skippedNoFile" | "skippedNoSection" | "skippedDisabled" | "failed";
  type RunParamsApplyStatus = { userLtx: ApplyOutcome; alifeLtx: ApplyOutcome };

  function applyStatusText(status: RunParamsApplyStatus): string {
    // user.ltx is only ever skipped when no installed version is active —
    // then nothing at all was patched, so this message wins.
    if (status.userLtx !== "applied") return $_("app.runParams.apply.noVersion");

    switch (status.alifeLtx) {
      case "applied":
        return "";
      case "skippedNoVersion":
        return $_("app.runParams.apply.noVersion");
      case "skippedNoFile":
        return $_("app.runParams.apply.alifeNoFile");
      case "skippedNoSection":
        return $_("app.runParams.apply.alifeNoSection");
      case "skippedDisabled":
        return $_("app.runParams.apply.alifeDisabled");
      default:
        return $_("app.runParams.apply.alifeFailed");
    }
  }

  // Состояния формы
  let fov = $state(50);
  let hudFov = $state(60);
  let launchArgs = $state("");
  let selectedResolution = $state("");
  let vsyncEnabled = $state(true);
  let selectedLang = $state(langMap.getValue(LangType.Rus) || "");
  let selectedRenderer = $state(renderersMap.getValue(RenderType.RendererR4) || "");
  let selectedScope = $state(scopesMap.getValue(ScopeType.Scopes2dStatic) || "");
  let presets = $state<IndexPreset[]>([]);
  let selectedPresetId = $state("");
  let applyPresetOnLaunch = $state(true);

  // User-editable A-Life settings; they override the preset values in alife.ltx
  let objectsPerUpdate = $state<number>(ALIFE_DEFAULTS.objects_per_update);
  let positionUpdateIntervalMs = $state<number>(ALIFE_DEFAULTS.position_update_interval_ms);
  let processTime = $state<number>(ALIFE_DEFAULTS.process_time);
  let switchDistance = $state<number>(ALIFE_DEFAULTS.switch_distance);

  // Флаги
  let uiDebug = $state(false);
  let checks = $state(false);
  let debugSpawn = $state(false);
  let useSpawner = $state(false);
  let noStaging = $state(false);
  let waitForKeypress = $state(true);
  let noPrefetch = $state(false);
  let windowedMode = $state(true);

  // Cheats / debug user.ltx props
  let godMode = $state(false);
  let unlimitedAmmo = $state(false);
  let showFps = $state(false);
  let showIds = $state(false);
  let fontLegacy = $state(false);

  // Моковые разрешения (замените на вызов Tauri команды позже)
  let resolutions = $state(["800x600 (60Hz)"]);
  let latestResolutions = $state("800x600 (60Hz)");

  function clearLaunchArgs() {
    launchArgs = "";
  }

  // Preset alife values come as strings from the index — parse, fall back to
  // the default and clamp into the UI range (a value beyond max would push
  // the TrackBar thumb outside the track).
  function num(raw: unknown, key: keyof typeof ALIFE_DEFAULTS): number {
    const range = ALIFE_RANGES[key];
    const parsed = typeof raw === "string" ? Number(raw) : raw;
    const value = typeof parsed === "number" && Number.isFinite(parsed) ? parsed : ALIFE_DEFAULTS[key];
    return Math.min(range.max, Math.max(range.min, value));
  }

  // Selecting a preset resets manual alife edits to the preset's values;
  // keys missing from the preset fall back to the defaults.
  function applyPresetToAlifeControls() {
    const alife = presets.find((p) => p.id === selectedPresetId)?.alife ?? {};
    objectsPerUpdate = num(alife.objects_per_update, "objects_per_update");
    positionUpdateIntervalMs = num(alife.position_update_interval_ms, "position_update_interval_ms");
    processTime = num(alife.process_time, "process_time");
    switchDistance = num(alife.switch_distance, "switch_distance");
  }

  async function handlePresetChange() {
    applyPresetToAlifeControls();
    if (selectedPresetId !== MAX_PERFORMANCE_PRESET_ID) return;
    if ($appConfig.hide_max_perf_preset_warning) return;
    $showDlgMaxPerformancePresetWarning = true;
  }

  async function handleSave() {
    const runParams: RunParams = {
      cmd_params: launchArgs,
      check_spawner: useSpawner,
      check_wait_press_any_key: waitForKeypress,
      check_without_cache: noPrefetch,
      check_vsync: vsyncEnabled,
      check_no_staging: noStaging,
      windowed_mode: windowedMode,
      ui_debug: uiDebug,
      checks,
      debug_spawn: debugSpawn,
      vid_mode: selectedResolution,
      lang: langMap.getKey(selectedLang) || LangType.Rus,
      render: renderersMap.getKey(selectedRenderer) || RenderType.RendererR4,
      fov,
      hud_fov: Number(Number(hudFov / 100).toFixed(2)),
      god_mode: godMode,
      unlimited_ammo: unlimitedAmmo,
      show_fps: showFps,
      show_ids: showIds,
      font_legacy: fontLegacy,
      scope_type: scopesMap.getKey(selectedScope) || ScopeType.Scopes2dStatic,
      selected_preset_id: selectedPresetId,
      apply_preset_on_launch: applyPresetOnLaunch,
      alife_objects_per_update: objectsPerUpdate,
      alife_position_update_interval_ms: positionUpdateIntervalMs,
      alife_process_time: processTime,
      alife_switch_distance: switchDistance,
      alife_overrides_initialized: true,
    };
    try {
      const status = await invoke<RunParamsApplyStatus>("update_run_params", { runParams });
      applyStatus = applyStatusText(status);
    } catch (e) {
      console.error("update_run_params failed:", e);
      applyStatus = $_("app.runParams.apply.error");

      return;
    }
    saving = true;
    setTimeout(() => (saving2 = true), 500);
    setTimeout(() => (saving = false), 1000);
    setTimeout(() => (saving2 = false), 1500);
  }

  $effect(() => {
    if ($configReady) {
      Promise.all([invoke<AppConfig>("get_config"), invoke<IndexPreset[]>("get_presets")]).then(([config, loadedPresets]) => {
        presets = loadedPresets;
        resolutions = config.vid_modes;
        latestResolutions = config.vid_mode_latest;
        launchArgs = config.run_params.cmd_params;
        useSpawner = config.run_params.check_spawner;
        waitForKeypress = config.run_params.check_wait_press_any_key;
        noPrefetch = config.run_params.check_without_cache;
        vsyncEnabled = config.run_params.check_vsync;
        noStaging = config.run_params.check_no_staging;
        windowedMode = config.run_params.windowed_mode;
        uiDebug = config.run_params.ui_debug;
        checks = config.run_params.checks;
        debugSpawn = config.run_params.debug_spawn;
        selectedResolution = config.run_params.vid_mode;
        selectedLang = langMap.getValue(config.run_params.lang as LangType)!;
        selectedRenderer = renderersMap.getValue(config.run_params.render as RenderType)!;
        fov = config.run_params.fov;
        hudFov = Math.round(config.run_params.hud_fov * 100);
        godMode = config.run_params.god_mode;
        unlimitedAmmo = config.run_params.unlimited_ammo;
        showFps = config.run_params.show_fps;
        showIds = config.run_params.show_ids;
        fontLegacy = config.run_params.font_legacy;
        selectedScope = scopesMap.getValue(config.run_params.scope_type as ScopeType)!;
        selectedPresetId = config.run_params.selected_preset_id || "";
        applyPresetOnLaunch = config.run_params.apply_preset_on_launch;
        objectsPerUpdate = num(config.run_params.alife_objects_per_update, "objects_per_update");
        positionUpdateIntervalMs = num(config.run_params.alife_position_update_interval_ms, "position_update_interval_ms");
        processTime = num(config.run_params.alife_process_time, "process_time");
        switchDistance = num(config.run_params.alife_switch_distance, "switch_distance");
        // Migration: a config saved before the overrides existed keeps the
        // preset's values in the controls until the first explicit save, so
        // what the user sees matches what actually goes into alife.ltx.
        if (!config.run_params.alife_overrides_initialized && selectedPresetId) {
          applyPresetToAlifeControls();
        }
      });
    }
  });

  // Reload the preset list once the release index refresh finishes — the
  // fresh index may carry new/updated presets. Only the dropdown list is
  // refreshed; form fields keep their current values.
  $effect(() => {
    if ($startupState.releases.status === "ok") {
      invoke<IndexPreset[]>("get_presets")
        .then((loadedPresets) => {
          presets = loadedPresets;
        })
        .catch((e) => console.error("get_presets refresh failed:", e));
    }
  });
</script>

<div class="launch-params-view">
  <h2>{$_("app.labels.runparams")}</h2>

  <Scroll value={240}>
    <!-- Поле для ключей запуска -->
    <Bg>
      <div class="input-row">
        <input type="text" bind:value={launchArgs} placeholder={$_("app.labels.runparams_holder")} class="launch-args-input" />
        <button type="button" onclick={clearLaunchArgs} class="clear-btn"> {$_("app.btn.clear")} </button>
      </div>
    </Bg>

    <div style="margin-bottom: 30px;" />

    <div class="optview">
      <div class="item">
        <Bg>
          <div class="opt">
            <span>
              {$_("app.params.screen")}
            </span>
            <div class="options-row">
              <label class="checkbox-label">
                <select bind:value={selectedResolution}>
                  {#each resolutions as res}
                    <option value={res}>{res}</option>
                  {/each}
                </select>
              </label>
            </div>
          </div>
          <div class="opt">
            <span>
              {$_("app.params.gameLang")}
            </span>
            <div class="options-row">
              <label class="checkbox-label">
                <select bind:value={selectedLang}>
                  {#each langMap as [type, name]}
                    <option value={name}>{name}</option>
                  {/each}
                </select>
              </label>
            </div>
          </div>
          <div class="opt">
            <span>
              {$_("app.params.renderer")}
            </span>
            <div class="options-row">
              <label class="checkbox-label">
                <select bind:value={selectedRenderer}>
                  {#each renderersMap as [type, name]}
                    <option value={name}>{$_(`app.renderer.${name}`)}</option>
                  {/each}
                </select>
              </label>
            </div>
          </div>
          {#if presets.length > 0}
            <div class="opt">
              <span>
                {$_("app.params.preset")}
              </span>
              <div class="options-row">
                <label class="checkbox-label">
                  <select bind:value={selectedPresetId} onchange={handlePresetChange}>
                    <option value="" disabled>{$_("app.presets.placeholder")}</option>
                    {#each presets as preset}
                      <option value={preset.id}>{$_(`app.presets.${preset.id}`)}</option>
                    {/each}
                  </select>
                </label>
              </div>
            </div>
            <label class="checkbox-label checkbox-label-preset-toggle">
              <input type="checkbox" bind:checked={applyPresetOnLaunch} />
              <span>{$_("app.params.applyPresetOnLaunch")}</span>
            </label>
          {/if}

          <div class="opt">
            <span>
              {$_("app.params.fov")}: {fov}
            </span>
            <div style="width: 100%">
              <TrackBar bind:value={fov} min={50} max={200} step={1} />
            </div>
          </div>
          <div class="opt">
            <span>
              {$_("app.params.hudFov")}: {hudFov}
            </span>
            <div style="width: 100%">
              <TrackBar bind:value={hudFov} min={10} max={100} step={1} />
            </div>
          </div>
          <div class="opt">
            <span>
              {$_("app.params.scopeType")}
            </span>
            <div class="scope-control">
              <div class="options-row">
                <label class="checkbox-label">
                  <select bind:value={selectedScope}>
                    {#each scopesMap as [type, name]}
                      <option value={name}>{$_(`app.scopes.${name}`)}</option>
                    {/each}
                  </select>
                </label>
              </div>
              <img src={`/static/g_3d_scopes/${selectedScope}.png`} alt={$_(`app.scopes.${selectedScope}`)} class="scope-preview-img" />
            </div>
          </div>
        </Bg>
      </div>
      <div class="item">
        <Bg>
          <label class="checkbox-label">
            <div class="opt check">
              <div class="checkbox-label">
                <input type="checkbox" bind:checked={windowedMode} />
              </div>
              <span>
                {$_("app.params.windowed")}
              </span>
            </div>
          </label>
          <label class="checkbox-label">
            <div class="opt check">
              <div class="checkbox-label">
                <input type="checkbox" bind:checked={vsyncEnabled} />
              </div>
              <span>
                {$_("app.params.vsync")}
              </span>
            </div>
          </label>
          <label class="checkbox-label">
            <div class="opt check">
              <div class="checkbox-label">
                <input type="checkbox" bind:checked={waitForKeypress} />
              </div>
              <span>
                {$_("app.params.presskey")}
              </span>
            </div>
          </label>
          <label class="checkbox-label">
            <div class="opt check">
              <div class="checkbox-label">
                <input type="checkbox" bind:checked={noStaging} />
              </div>
              <span>
                {$_("app.params.nostaging")}
              </span>
            </div>
          </label>
          <label class="checkbox-label">
            <div class="opt check">
              <div class="checkbox-label">
                <input type="checkbox" bind:checked={noPrefetch} />
              </div>
              <span>
                {$_("app.params.noprefetch")}
              </span>
            </div>
          </label>
          <label class="checkbox-label">
            <div class="opt check">
              <div class="checkbox-label">
                <input type="checkbox" bind:checked={useSpawner} />
              </div>
              <span>
                {$_("app.params.dbg")}
              </span>
            </div>
          </label>
          <label class="checkbox-label">
            <div class="opt check">
              <div class="checkbox-label">
                <input type="checkbox" bind:checked={uiDebug} />
              </div>
              <span>
                {$_("app.params.uidbg")}
              </span>
            </div>
          </label>
          <label class="checkbox-label">
            <div class="opt check">
              <div class="checkbox-label">
                <input type="checkbox" bind:checked={checks} />
              </div>
              <span>
                {$_("app.params.checks")} <span class="warntext">{$_("app.params.checksnote")}</span>
              </span>
            </div>
          </label>
          <label class="checkbox-label">
            <div class="opt check">
              <div class="checkbox-label">
                <input type="checkbox" bind:checked={debugSpawn} />
              </div>
              <span>
                {$_("app.params.dbgsspwn")}
              </span>
            </div>
          </label>
          <label class="checkbox-label">
            <div class="opt check">
              <div class="checkbox-label">
                <input type="checkbox" bind:checked={godMode} />
              </div>
              <span>
                {$_("app.params.godMode")}
              </span>
            </div>
          </label>
          <label class="checkbox-label">
            <div class="opt check">
              <div class="checkbox-label">
                <input type="checkbox" bind:checked={unlimitedAmmo} />
              </div>
              <span>
                {$_("app.params.unlimitedAmmo")}
              </span>
            </div>
          </label>
          <label class="checkbox-label">
            <div class="opt check">
              <div class="checkbox-label">
                <input type="checkbox" bind:checked={showFps} />
              </div>
              <span>
                {$_("app.params.showFps")}
              </span>
            </div>
          </label>
          <label class="checkbox-label">
            <div class="opt check">
              <div class="checkbox-label">
                <input type="checkbox" bind:checked={showIds} />
              </div>
              <span>
                {$_("app.params.showIds")}
              </span>
            </div>
          </label>
          <label class="checkbox-label">
            <div class="opt check">
              <div class="checkbox-label">
                <input type="checkbox" bind:checked={fontLegacy} />
              </div>
              <span>
                {$_("app.params.fontLegacy")}
              </span>
            </div>
          </label>
        </Bg>
      </div>
      <div class="item item-alife">
        <Bg>
          <div class="alife-header">
            <span class="alife-title">{$_("app.params.alifeSection")}</span>
            {#if applyPresetOnLaunch}
              <span class="warntext">{$_("app.params.alifeNote")}</span>
            {:else}
              <span class="warntext">{$_("app.params.alifeDisabledNote")}</span>
            {/if}
          </div>
          <div class="opt alife-opt">
            <span>
              {$_("app.params.objectsPerUpdate")}: {objectsPerUpdate}
            </span>
            <div style="width: 100%">
              <TrackBar
                bind:value={objectsPerUpdate}
                min={ALIFE_RANGES.objects_per_update.min}
                max={ALIFE_RANGES.objects_per_update.max}
                step={ALIFE_RANGES.objects_per_update.step}
              />
            </div>
          </div>
          <div class="opt alife-opt">
            <span>
              {$_("app.params.positionUpdateIntervalMs")}: {positionUpdateIntervalMs}
            </span>
            <div style="width: 100%">
              <TrackBar
                bind:value={positionUpdateIntervalMs}
                min={ALIFE_RANGES.position_update_interval_ms.min}
                max={ALIFE_RANGES.position_update_interval_ms.max}
                step={ALIFE_RANGES.position_update_interval_ms.step}
              />
            </div>
          </div>
          <div class="opt alife-opt">
            <span>
              {$_("app.params.processTime")}: {processTime}
            </span>
            <div style="width: 100%">
              <TrackBar
                bind:value={processTime}
                min={ALIFE_RANGES.process_time.min}
                max={ALIFE_RANGES.process_time.max}
                step={ALIFE_RANGES.process_time.step}
              />
            </div>
          </div>
          <div class="opt alife-opt">
            <span>
              {$_("app.params.switchDistance")}: {switchDistance}
            </span>
            <div style="width: 100%">
              <TrackBar
                bind:value={switchDistance}
                min={ALIFE_RANGES.switch_distance.min}
                max={ALIFE_RANGES.switch_distance.max}
                step={ALIFE_RANGES.switch_distance.step}
              />
            </div>
          </div>
        </Bg>
      </div>
    </div>
  </Scroll>

  <!-- Кнопка сохранения -->
  {#if applyStatus}
    <span class="apply-status">{applyStatus}</span>
  {/if}
  <span role="button" tabindex="0" onclick={handleSave} class="save-btn" class:save-btn__saving={saving} class:long_t={saving2}>
    {#if saving}
      {$_("app.save.2")}
    {:else}
      {$_("app.save.1")}
    {/if}
  </span>
</div>

<style>
  h2 {
    margin-bottom: 4rem;
  }

  .optview {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }
  .item {
    flex: 1 1 600px;
  }
  /* The A-Life block takes a deliberate full row instead of dangling alone
     next to the two half-width blocks. */
  .item-alife {
    flex: 1 1 100%;
  }
  .opt {
    display: grid;
    grid-template-columns: 14vw 1fr;
    margin-bottom: 14px;
  }
  /* Long alife labels ("Интервал обновления позиций (мс)") do not fit the
     default 14vw column without wrapping. */
  .alife-opt {
    grid-template-columns: minmax(230px, 22vw) 1fr;
  }
  .check {
    grid-template-columns: 4vw 1fr;
  }
  .opt > span {
    justify-self: end;
    padding-right: 14px;
    align-self: center;
  }
  .opt > div {
    justify-self: start;
    align-self: center;
  }
  .check > span {
    justify-self: start;
  }
  .check > div {
    padding-right: 20px;
    justify-self: end;
  }

  .launch-params-view {
    padding: 1.5rem;
    margin: 0 auto;
    font-family: system-ui, sans-serif;
  }

  .input-row {
    -webkit-app-region: no-drag;
    display: flex;
    gap: 0.75rem;
  }

  .launch-args-input {
    -webkit-app-region: no-drag;
    flex: 1;
    padding: 0.5rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    background-color: rgba(255, 255, 255, 0.8);
  }
  .launch-args-input:focus {
    background-color: rgba(255, 255, 255, 1);
  }

  .clear-btn {
    -webkit-app-region: no-drag;
    padding: 0.6rem 1.6rem;
    color: #fff;
    background-color: rgba(61, 93, 236, 0.8);
    border: 0px solid #ccc;
    border-radius: 3px;
    cursor: pointer;
    transition: background-color 0.15s ease;
  }

  .clear-btn:hover {
    background-color: rgba(61, 93, 236, 1);
  }

  .warntext {
    font-size: 0.8rem;
    color: rgba(252, 186, 186, 0.8);
  }

  .alife-header {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 10px;
  }

  .alife-title {
    font-weight: 600;
  }

  .options-row {
    -webkit-app-region: no-drag;
    display: flex;
    flex-wrap: nowrap;
    gap: 3.5rem;
  }

  .options-row label {
    -webkit-app-region: no-drag;
    display: flex;
    flex-direction: row;
    gap: 1.25rem;
    text-wrap: nowrap;
  }

  .options-row select {
    -webkit-app-region: no-drag;
    padding: 0.4rem 0.6rem;
    font-size: 1rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    background-color: rgba(255, 255, 255, 0.8);
  }
  .options-row select:focus {
    background-color: rgba(255, 255, 255, 1);
  }

  .tracks-row {
    -webkit-app-region: no-drag;
    display: flex;
    flex-wrap: nowrap;
  }

  .checkbox-label {
    -webkit-app-region: no-drag;
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .checkbox-label:hover {
    cursor: pointer;
  }

  .checkbox-label-preset-toggle {
    justify-content: center;
    margin-bottom: 14px;
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

  .flags-section {
    margin-bottom: 2rem;
  }

  /* Sits right above the save button (which is absolutely positioned too),
     so the line never pushes the layout or lands under the button. */
  .apply-status {
    position: absolute;
    bottom: 100px;
    right: 140px;
    max-width: 60vw;
    text-align: right;
    color: #f5a623;
  }

  .save-btn {
    -webkit-app-region: no-drag;
    position: absolute;
    bottom: 50px;
    right: 140px;
    padding: 10px 40px;
    color: white;
    border-radius: 3px;
    background-color: rgba(61, 93, 236, 0.8);
    transition: background-color 0.15s ease;
  }
  .save-btn:hover {
    background-color: rgba(61, 93, 236, 1);
  }
  .save-btn__saving {
    background-color: rgba(61, 236, 128, 0.8);
  }
  .save-btn__saving:hover {
    background-color: rgba(61, 236, 128, 0.8);
  }
  .long_t {
    transition: background-color 1s ease;
  }

  .opt:has(.scope-control) > span {
    align-self: start;
    padding-top: 0.5rem;
  }

  .scope-control {
    display: flex;
    flex-wrap: nowrap;
    align-items: flex-start;
    gap: 12px;
  }

  .scope-preview-img {
    width: 256px;
    height: 256px;
    aspect-ratio: 1 / 1;
    object-fit: contain;
    border-radius: 4px;
    border: 1px solid rgba(255, 255, 255, 0.2);
    background: rgba(0, 0, 0, 0.35);
    flex-shrink: 0;
  }
</style>
