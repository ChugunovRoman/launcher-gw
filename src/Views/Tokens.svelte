<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { loadedTokens, providersWasInited, tokens } from "../store/main";

  // Tokens are write-only now: get_tokens() returns masked values only,
  // the input is used exclusively to enter a NEW token.
  let tokenMasks: Record<string, string> = $state({});
  let saving: Record<string, boolean> = $state({});

  function updateToken(id: string, newValue: string) {
    tokens.update((m) => {
      const next = new Map(m);
      next.set(id, newValue);
      return next;
    });
  }

  async function refreshMasks() {
    try {
      const tokensMap = await invoke<Dict<string>>("get_tokens");
      for (const [id, mask] of Object.entries(tokensMap)) {
        tokenMasks[id] = mask || "";
      }
    } catch (e) {
      console.error("Failed to load token masks:", e);
    }
  }

  async function saveToken(id: string) {
    const value = $tokens.get(id) || "";
    if (!value) return;

    saving[id] = true;
    try {
      await invoke("set_token_for_provider", {
        token: value,
        providerId: id,
      });
      updateToken(id, "");
      await refreshMasks();
    } catch (e) {
      console.error(`Failed to save token for ${id}:`, e);
    } finally {
      saving[id] = false;
    }
  }

  async function loadProviders() {
    try {
      const providerIds = await invoke<string[]>("get_provider_ids");
      const tokensMap = await invoke<Dict<string>>("get_tokens");

      for (const id of providerIds) {
        updateToken(id, "");
        tokenMasks[id] = tokensMap[id] || "";
      }

      loadedTokens.set(true);
    } catch (e) {
      console.error("Failed to load providers:", e);
    }
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
      {#if tokenMasks[id]}
        <div class="current-token">{$_("app.tokens.current")}: {tokenMasks[id]}</div>
      {/if}
      <div class="input-row">
        <input
          type="password"
          value={token}
          oninput={(e: any) => updateToken(id, e.target.value)}
          placeholder={$_(`app.tokens.label.${id}`)}
          disabled={saving[id]}
          class="token-input" />
        <button type="button" onclick={() => saveToken(id)} class="choose-btn" disabled={!token || saving[id]}>
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

  .current-token {
    margin-bottom: 0.5rem;
    color: #bbb;
    font-size: 0.9rem;
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

  .choose-btn:hover:not(:disabled) {
    background-color: rgba(61, 93, 236, 1);
  }

  .choose-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
