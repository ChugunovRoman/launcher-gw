<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { loadedTokens, providersWasInited, tokens } from "../store/main";

  let visiblePasswords: Record<string, boolean> = $state({});

  function toggleVisibility(id: string) {
    visiblePasswords[id] = !visiblePasswords[id];
  }

  function updateToken(id: string, newValue: string) {
    $tokens.set(id, newValue);
    tokens.update((m) => {
      m.set(id, newValue);
      return m;
    });
  }
  async function saveToken(id: string) {
    const value = $tokens.get(id);
    await invoke<string[]>("set_token_for_provider", {
      token: value,
      providerId: id,
    });
  }

  async function loadProviders() {
    const providerIds = await invoke<string[]>("get_provider_ids");
    const tokensMap = await invoke<Dict<string>>("get_tokens");

    for (const id of providerIds) {
      updateToken(id, tokensMap[id] || "");
      visiblePasswords[id] = false;
    }

    loadedTokens.set(true);
  }

  $effect(() => {
    if ($providersWasInited && !$loadedTokens) {
      loadProviders();
    }
  });
</script>

<div class="releases-view">
  <h2>{$_("app.labels.tokens")}</h2>

  {#each Array.from($tokens.entries()) as [id, token], index}
    <div class="input-group">
      <label class="input-label">{$_(`app.tokens.label.${id}`)}</label>
      <div class="input-row">
        <input
          type={visiblePasswords[id] ? "text" : "password"}
          value={token}
          oninput={(e: any) => updateToken(id, e.target.value)}
          placeholder={$_(`app.tokens.label.${id}`)}
          class="token-input" />
        <button type="button" class="toggle-visibility-btn" onclick={() => toggleVisibility(id)}>
          {visiblePasswords[id] ? $_("app.tokens.hide") : $_("app.tokens.show")}
        </button>
        <button type="button" onclick={() => saveToken(id)} class="choose-btn">
          {$_("app.tokens.save")}
        </button>
      </div>
    </div>
  {/each}
</div>

<style>
  h2 {
    margin-bottom: 4rem;
  }

  .releases-view {
    padding: 1.5rem;
    margin: 0 auto;
    font-family: system-ui, sans-serif;
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

  .toggle-visibility-btn {
    -webkit-app-region: no-drag;
    padding: 0.5rem 0.75rem;
    color: #fff;
    background-color: rgba(100, 100, 100, 0.8);
    border: none;
    border-radius: 3px;
    cursor: pointer;
    transition: background-color 0.15s ease;
    white-space: nowrap;
  }

  .toggle-visibility-btn:hover {
    background-color: rgba(120, 120, 120, 1);
  }

  .input-row {
    -webkit-app-region: no-drag;
    display: flex;
    gap: 0.5rem;
    margin-bottom: 2.5rem;
  }
  .token-input {
    -webkit-app-region: no-drag;
    flex: 1;
    padding: 0.5rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    background-color: rgba(255, 255, 255, 0.8);
  }
  .token-input:focus {
    background-color: rgba(255, 255, 255, 1);
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
</style>
