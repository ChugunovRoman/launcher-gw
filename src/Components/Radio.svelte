<script lang="ts">
  let { children, value = $bindable(""), name = $bindable(""), group = $bindable(""), disabled = $bindable(false) } = $props();
</script>

<label class="checkbox-label radio-variant" class:radio-disabled={disabled}>
  <input {disabled} type="radio" {name} {value} bind:group />
  {@render children?.()}
</label>

<style>
  .checkbox-label {
    -webkit-app-region: no-drag;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    backdrop-filter: blur(2px);
    padding: 2px 8px;
    margin-left: 10px;
  }
  .checkbox-label:hover {
    cursor: pointer;
  }

  .radio-variant input[type="radio"] {
    -webkit-app-region: no-drag;
    appearance: none;
    width: 20px;
    height: 20px;
    border: 10px solid #007acc;
    border-radius: 50%;
    background: rgba(30, 30, 30, 0.8);
    outline: none;
    cursor: pointer;
    position: relative;
    transition: background 0.2s ease;
    margin-left: 10px;
  }

  /* Синий кружок внутри */
  .radio-variant input[type="radio"]::after {
    content: "";
    position: absolute;
    top: 50%;
    left: 50%;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: white; /* Синий цвет */
    opacity: 0;
    transform: translate(-50%, -50%) scale(0.8);
    transition:
      opacity 0.25s ease,
      transform 0.25s ease;
  }

  /* Показываем кружок, когда чекбокс checked */
  .radio-variant input[type="radio"]:checked::after {
    opacity: 1;
    transform: translate(-50%, -50%) scale(1);
  }

  /* Опционально: hover-эффект */
  .radio-variant input[type="radio"]:hover {
    background: rgba(40, 40, 40, 0.7);
  }

  .radio-disabled input[type="radio"] {
    background: rgba(40, 40, 40, 0.7);
    border: 10px solid #3e515d;
  }
</style>
