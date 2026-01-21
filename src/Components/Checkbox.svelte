<script lang="ts">
  import type { HTMLAttributes } from "svelte/elements";
  interface Props extends HTMLAttributes<HTMLInputElement> {
    checked: boolean;
  }

  let { children, checked = $bindable(false), ...rest }: Props = $props();
</script>

<label class="checkbox-label">
  <div class="opt check">
    <div class="checkbox-label">
      <input type="checkbox" bind:checked {...rest} />
    </div>
    <span>
      {@render children?.()}
    </span>
  </div>
</label>

<style>
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
</style>
