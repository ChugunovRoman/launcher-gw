<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { save, open } from "@tauri-apps/plugin-dialog";
  import { join } from "@tauri-apps/api/path";
  import {
    profiles,
    profileKeyMap,
    applyKeyProfile,
    selectedProfile,
    showDlgRemoveProfile,
    removeProfileName,
    selectedProfileMap,
    updateCurrentBindsMap,
    showDlgApplyProfile,
    showDlgApplyProfileOk,
  } from "../store/profiles";
  import { DEFAULT_BIND_LTX, KEYS_MAP, NO_KEY } from "../consts";

  import Scroll from "../Components/Scroll.svelte";
  import SelectGroup from "../Components/SelectGroup.svelte";
  import Button from "../Components/Button.svelte";
  import Bg from "../Components/Bg.svelte";
  import Checkbox from "../Components/Checkbox.svelte";
  import { sortOptions, transformToKeymapArray } from "../lib/profiles";

  let nameExist = $state(false);
  let renameDefaultError = $state(false);
  let saving = $state(false);
  let saving2 = $state(false);
  let selectedProfileName = $state("");

  function addProfileHandler() {
    nameExist = false;

    if ($profiles.find((p) => p.label === selectedProfileName || p.value === selectedProfileName)) {
      nameExist = true;
      return;
    }

    const name = `${selectedProfileName}.ltx`;
    profileKeyMap.setItem(name, $profileKeyMap.get($selectedProfile!)!);
    profiles.push({
      label: selectedProfileName.replace(".ltx", ""),
      value: name,
    });
    updateCurrentBindsMap();
    sortOptions();

    invoke<void>("add_profile", { name, basedOnProfile: $selectedProfile });
  }

  function handleCheckApply() {
    invoke<void>("set_apply_profile", { profileName: $selectedProfile || "", apply: $applyKeyProfile });
  }
  function renameHandler() {
    nameExist = false;

    const oldName = $selectedProfile!;
    if (oldName === DEFAULT_BIND_LTX) {
      renameDefaultError = true;
      setTimeout(() => {
        renameDefaultError = false;
      }, 5000);
      return;
    }
    if ($profiles.find((p) => p.label === selectedProfileName || p.value === selectedProfileName)) {
      nameExist = true;
      return;
    }

    const name = `${selectedProfileName.replace(".ltx", "")}.ltx`;
    const oldValue = $profileKeyMap.get(oldName)!;
    profileKeyMap.delItem(oldName);
    profileKeyMap.setItem(name, oldValue);
    profiles.set($profiles.filter((p) => p.value !== oldName));
    profiles.push({
      label: selectedProfileName.replace(".ltx", ""),
      value: name,
    });
    selectedProfile.set(name);
    updateCurrentBindsMap();
    sortOptions();

    invoke<void>("rename_profile", { oldName, newName: name });
  }

  async function handleSave() {
    const payload: ProfileItem[] = [];

    // Итерируемся по именам всех профилей, которые у нас есть
    for (const profileName of $profileKeyMap.keys()) {
      const keymapArray = $profileKeyMap.get(profileName)!;

      const keybindsMap: Record<string, any> = {};
      keymapArray.forEach((item) => {
        keybindsMap[item.action] = {
          key: item.key,
          altkey: item.altkey,
        };
      });

      payload.push({
        name: profileName,
        keybinds: keybindsMap,
      });
    }

    try {
      await invoke("save_key_profiles", { profiles: payload });
      console.log("All profiles saved successfully");
    } catch (err) {
      console.error("Failed to save profiles:", err);
    }

    saving = true;
    setTimeout(() => (saving2 = true), 500);
    setTimeout(() => (saving = false), 1000);
    setTimeout(() => (saving2 = false), 1500);
  }
  async function handleApplyToOther() {
    const path = await open({ multiple: false, directory: true });

    if (!path || Array.isArray(path)) return;

    const userltxPath = await join(path, "appdata", "user.ltx");

    // Проверяем существование
    const fileExists = await invoke("check_file_exists", { path: userltxPath });
    if (!fileExists) {
      showDlgApplyProfile.set(true);
      return;
    }

    await invoke<void>("apply_profile_to_ltx", { profileName: $selectedProfile, ltxPath: userltxPath });

    showDlgApplyProfileOk.set(true);
  }
  async function handleImport() {
    try {
      // 1. Открываем диалог выбора файла
      const path = await open({
        multiple: false,
        filters: [
          {
            name: "Keybind Profile",
            extensions: ["ltx"],
          },
        ],
      });

      if (!path || Array.isArray(path)) return; // Если путь не выбран или выбран массив

      // 2. Вызываем команду импорта
      const newProfile: ProfileItem = await invoke("import_profile", { path });

      // 3. Обновляем локальное состояние
      console.log("Профиль импортирован:", newProfile);

      profileKeyMap.setItem(newProfile.name, transformToKeymapArray(newProfile.keybinds));
      profiles.push({ label: newProfile.name.replace(".ltx", ""), value: newProfile.name });

      updateCurrentBindsMap();
      sortOptions();

      return newProfile;
    } catch (err) {
      console.error("Ошибка при импорте:", err);
    }
  }
  async function handleExport() {
    try {
      // 1. Открываем диалог сохранения файла
      const path = await save({
        filters: [
          {
            name: "Keybind Profile",
            extensions: ["ltx"],
          },
        ],
        defaultPath: $selectedProfile,
      });

      if (!path) return; // Пользователь отменил выбор

      // 2. Вызываем команду экспорта
      await invoke("export_profile", { name: $selectedProfile, path });
      console.log("Экспорт завершен успешно");
    } catch (err) {
      console.error("Ошибка при экспорте:", err);
    }
  }
  async function handleRemove(option: Option) {
    removeProfileName.set(option.value);
    showDlgRemoveProfile.set(true);
  }

  // Состояние для редактирования: [имя_действия, индекс_кнопки (0 или 1)]
  let editingTarget = $state<{ action: string; index: number } | null>(null);

  // Функция для захвата клавиши
  function handleGlobalInput(event: KeyboardEvent | MouseEvent) {
    if (!editingTarget) return;

    // Отмена на Esc
    if (event instanceof KeyboardEvent && event.code === "Escape") {
      editingTarget = null;
      return;
    }
    if (event instanceof KeyboardEvent && event.code === "Delete") {
      updateBinding(editingTarget.action, editingTarget.index, NO_KEY);
      editingTarget = null;
      return;
    }

    event.preventDefault();
    let pressedKey = "";

    if (event instanceof KeyboardEvent) {
      // event.code возвращает KeyQ, Digit1, Space и т.д. независимо от языка
      pressedKey = KEYS_MAP[event.code] || event.code;
    } else if (event instanceof MouseEvent) {
      // Кодируем кнопки мыши (0: Left, 1: Middle, 2: Right)
      pressedKey = KEYS_MAP[`Mouse${event.button}`] || `Mouse${event.button}`;
    }

    if (pressedKey) {
      updateBinding(editingTarget.action, editingTarget.index, pressedKey);
      editingTarget = null;
    }
  }

  function updateBinding(action: string, index: number, newKey: string) {
    if (!$selectedProfile) {
      return;
    }

    const currentProfileName = $selectedProfile;
    const currentMap = $profileKeyMap.get(currentProfileName);

    if (currentMap) {
      let bindingIndex = currentMap.findIndex((b) => b.action === action);
      if (bindingIndex === -1) {
        bindingIndex = currentMap.push({ action, key: index === 0 ? newKey : NO_KEY, altkey: index === 1 ? newKey : NO_KEY });
      } else {
        if (index === 0) currentMap[bindingIndex].key = newKey;
        else currentMap[bindingIndex].altkey = newKey;
      }

      // Сохраняем в store (предполагая, что это Svelte store с методом setItem или через обновление значения)
      profileKeyMap.setItem(currentProfileName, currentMap);
      updateCurrentBindsMap();
    }
  }

  // Сброс при потере фокуса или выходе за границы
  function stopEditing() {
    editingTarget = null;
  }

  $effect(() => {
    $selectedProfile = $selectedProfile;
    if ($selectedProfile) {
      selectedProfileName = $selectedProfile.replace(".ltx", "");
      updateCurrentBindsMap();
    }
  });

  onMount(() => {
    updateCurrentBindsMap();
  });
</script>

<svelte:window onkeydown={editingTarget ? handleGlobalInput : null} onmousedown={editingTarget ? handleGlobalInput : null} onblur={stopEditing} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="launch-params-view" onmouseleave={stopEditing}>
  <h2>{$_("app.menu.keybindings")}</h2>

  <Bg>
    <Checkbox bind:checked={$applyKeyProfile} onchange={handleCheckApply}>
      {$_("app.params.applyKeyProfile")}
    </Checkbox>
    <div class="input-row">
      <input type="text" bind:value={selectedProfileName} placeholder={$_("app.labels.profile_name_holder")} class="launch-args-input" />
      <Button onclick={renameHandler}>{$_("app.btn.rename")}</Button>
      <Button onclick={addProfileHandler}>{$_("app.btn.add")}</Button>
    </div>
    {#if nameExist}
      <span class="input-label-2">{$_("app.input.checks.profileExists")}</span>
    {/if}
    {#if renameDefaultError}
      <span class="input-label-2">{$_("app.input.checks.renameDefaultError")}</span>
    {/if}
  </Bg>

  <div class="cols">
    <Bg>
      <div class="table" style="padding-right: 33px;">
        <!-- header -->
        <div class="table_header"><span class="bold">{$_("app.keys.header.action")}</span></div>
        <div class="table_header"><span class="bold">{$_("app.keys.header.key")}</span></div>
        <div class="table_header"><span class="bold">{$_("app.keys.header.altkey")}</span></div>
      </div>
      <Scroll value={450}>
        <div class="table">
          <!-- actions -->
          {#if $profileKeyMap.has($selectedProfile!)}
            {#each $selectedProfileMap as [groupName, list]}
              <div class="table_item"><br /></div>
              <div class="table_item"><br /></div>
              <div class="table_item"><br /></div>

              <div class="table_item"><span class="bold">{$_(`app.keys.group.${groupName}`, { default: groupName })}:</span></div>
              <div class="table_item"></div>
              <div class="table_item"></div>
              {#each Object.keys(list) as action}
                <div class="table_item" class:disabled={$selectedProfile === DEFAULT_BIND_LTX}>
                  {$_(`app.keys.actions.${action}`, { default: action })}
                </div>
                <div
                  class="table_item bind-cell"
                  class:disabled={$selectedProfile === DEFAULT_BIND_LTX}
                  class:blinking={$selectedProfile !== DEFAULT_BIND_LTX && editingTarget?.action === action && editingTarget?.index === 0}
                  onclick={() => ($selectedProfile === DEFAULT_BIND_LTX ? 0 : (editingTarget = { action, index: 0 }))}
                  role="button"
                  tabindex="0">
                  {list[action][0]}
                </div>

                <div
                  class="table_item bind-cell"
                  class:disabled={$selectedProfile === DEFAULT_BIND_LTX}
                  class:blinking={$selectedProfile !== DEFAULT_BIND_LTX && editingTarget?.action === action && editingTarget?.index === 1}
                  onclick={() => ($selectedProfile === DEFAULT_BIND_LTX ? 0 : (editingTarget = { action, index: 1 }))}
                  role="button"
                  tabindex="0">
                  {list[action][1]}
                </div>
              {/each}
            {/each}
          {/if}
        </div>
        <div style="margin-bottom: 30px;" />
      </Scroll>
    </Bg>
    <Bg>
      <Scroll value={450}>
        <SelectGroup
          options={$profiles}
          bind:value={$selectedProfile}
          ondelete={handleRemove}
          excludeDeleteFor={[DEFAULT_BIND_LTX]}
          name="key-profiles" />
      </Scroll>
    </Bg>
  </div>

  <!-- Кнопка сохранения -->
  <div class="btn-bar">
    <Button onclick={handleApplyToOther}>{$_("app.btn.applyToOther")}</Button>
    <div style="padding-left: 10px;"></div>
    <Button onclick={handleImport}>{$_("app.btn.import")}</Button>
    <div style="padding-left: 10px;"></div>
    <Button onclick={handleExport}>{$_("app.btn.export")}</Button>
    <div style="padding-left: 10px;"></div>
    <span role="button" tabindex="0" onclick={handleSave} class="save-btn" class:save-btn__saving={saving} class:long_t={saving2}>
      {#if saving}
        {$_("app.save.2")}
      {:else}
        {$_("app.save.1")}
      {/if}
    </span>
  </div>
</div>

<style>
  h2 {
    margin-bottom: 4rem;
  }

  .cols {
    display: grid;
    grid-template-columns: 1fr 340px;
    grid-column-gap: 10px;
  }

  .table {
    display: grid;
    grid-template-columns: 1fr 120px 120px;
    overflow-x: clip;
    overflow-y: auto;
  }
  .table_header {
    display: flex;
  }
  .table_item {
    display: flex;
  }

  .bold {
    font-weight: 600;
    font-size: large;
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

  .input-label-2 {
    display: block;
    margin-bottom: 0.5rem;
    color: #f55858;
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

  .btn-bar {
    display: flex;
    position: absolute;
    bottom: 50px;
    right: 40px;
  }

  .bind-cell {
    cursor: pointer;
    padding: 2px 5px;
    border: 1px solid transparent;
    transition: background 0.2s;
    user-select: none;
  }

  .bind-cell:hover {
    background: rgba(255, 255, 255, 0.1);
    border-color: #666;
  }

  .blinking {
    animation: blinker 1s linear infinite;
    background: rgba(61, 93, 236, 0.3) !important;
    color: #fff;
  }
  .disabled {
    color: rgba(180, 180, 180, 0.8);
  }
  .disabled:hover {
    cursor: default;
    animation: none;
    color: rgba(180, 180, 180, 0.8);
    background: transparent;
    border-color: 0;
    border: 0;
  }

  @keyframes blinker {
    50% {
      opacity: 0.3;
    }
  }

  .save-btn {
    -webkit-app-region: no-drag;
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
