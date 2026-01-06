<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { providersWasInited } from "../store/main";
  import { LangType, RenderType } from "../consts";
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

  let saving = $state(false);
  let saving2 = $state(false);

  // Состояния формы
  let fov = $state(50);
  let hudFov = $state(60);
  let launchArgs = $state("");
  let selectedResolution = $state("");
  let vsyncEnabled = $state(true);
  let selectedLang = $state(langMap.getValue(LangType.Rus) || "");
  let selectedRenderer = $state(renderersMap.getValue(RenderType.RendererR4) || "");

  // Флаги
  let uiDebug = $state(false);
  let checks = $state(false);
  let debugSpawn = $state(false);
  let useSpawner = $state(false);
  let noStaging = $state(false);
  let waitForKeypress = $state(true);
  let noPrefetch = $state(false);
  let windowedMode = $state(true);

  // Моковые разрешения (замените на вызов Tauri команды позже)
  let resolutions = $state(["800x600 (60Hz)"]);
  let latestResolutions = $state("800x600 (60Hz)");

  function clearLaunchArgs() {
    launchArgs = "";
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
    };
    await invoke<void>("update_run_params", { runParams });
    saving = true;
    setTimeout(() => (saving2 = true), 500);
    setTimeout(() => (saving = false), 1000);
    setTimeout(() => (saving2 = false), 1500);
  }

  $effect(() => {
    if ($providersWasInited) {
      invoke<AppConfig>("get_config").then((config) => {
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
        hudFov = Math.floor(config.run_params.hud_fov * 100);
      });
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
        <button type="button" onclick={clearLaunchArgs} class="clear-btn"> {$_("app.clear")} </button>
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
        </Bg>
      </div>
    </div>
  </Scroll>

  <!-- Кнопка сохранения -->
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
  .opt {
    display: grid;
    grid-template-columns: 14vw 1fr;
    margin-bottom: 14px;
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
</style>
