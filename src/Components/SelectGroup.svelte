<script lang="ts">
  import { Trash2 } from "lucide-svelte";

  let { options = [] as Option[], value = $bindable(), name = "radio-group", ondelete = () => {}, excludeDeleteFor = [] } = $props();
</script>

<div class="select-group">
  {#each options as option}
    <div class="item">
      <label class="item-label">
        <div class="radio-wrapper">
          <input type="radio" {name} value={option.value} bind:group={value} />
        </div>
        <span class="label-text">{option.label}</span>
      </label>
      {#if !excludeDeleteFor.includes(option.value)}
        <div class="icon">
          <Trash2 size={22} onclick={() => ondelete(option)} />
        </div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .select-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 100%;
  }

  .item {
    display: flex;
    gap: 10px;
  }
  .icon {
    margin-left: auto;
  }
  .icon:hover {
    cursor: pointer;
  }
  .item-label {
    -webkit-app-region: no-drag;
    display: flex;
    align-items: center;
    gap: 12px;
    cursor: pointer;
    padding: 4px 0;
    transition: opacity 0.2s;
  }

  .item-label:hover {
    opacity: 0.8;
  }

  /* Стилизация под ваши чекбоксы */
  .radio-wrapper input[type="radio"] {
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
    display: block;
  }

  .radio-wrapper input[type="radio"]::after {
    content: "";
    position: absolute;
    top: 50%;
    left: 50%;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #007acc; /* Ваш синий цвет */
    opacity: 0;
    transform: translate(-50%, -50%) scale(0.8);
    transition:
      opacity 0.25s ease,
      transform 0.25s ease;
  }

  .radio-wrapper input[type="radio"]:checked::after {
    opacity: 1;
    transform: translate(-50%, -50%) scale(1);
  }

  .label-text {
    color: white;
    font-size: 0.95rem;
    user-select: none;
  }
</style>
