<script lang="ts">
  let { value = $bindable(0), min = 0, max = 100, step = 1 } = $props();

  let isDragging = $state(false);
  let barElement: HTMLDivElement | undefined = $state();

  // Вычисляем процент заполнения для позиционирования кружка
  let percentage = $derived(((value - min) / (max - min)) * 100);

  function updateValue(clientX: number) {
    if (!barElement) return;

    const rect = barElement.getBoundingClientRect();
    const offsetX = clientX - rect.left;
    const rawPercentage = Math.max(0, Math.min(1, offsetX / rect.width));

    let newValue = min + rawPercentage * (max - min);

    // Применяем шаг
    value = Math.round(newValue / step) * step;
  }

  function onMouseDown(e: MouseEvent) {
    isDragging = true;
    updateValue(e.clientX);

    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
  }

  function onMouseMove(e: MouseEvent) {
    if (isDragging) updateValue(e.clientX);
  }

  function onMouseUp() {
    isDragging = false;
    window.removeEventListener("mousemove", onMouseMove);
    window.removeEventListener("mouseup", onMouseUp);
  }
</script>

<div class="track-container">
  <div class="track-bar" bind:this={barElement} onmousedown={onMouseDown}>
    <div class="track-line"></div>

    <div class="track-thumb" style:left="{percentage}%" class:active={isDragging}></div>
  </div>
</div>

<style>
  .track-container {
    width: 100%;
    padding: 20px 0;
    user-select: none;
  }

  .track-bar {
    position: relative;
    width: 100%;
    height: 4px;
    cursor: pointer;
    display: flex;
    align-items: center;
  }

  .track-line {
    position: absolute;
    width: 100%;
    height: 100%;
    background-color: white;
    border-radius: 2px;
    /* Можно добавить легкую тень, чтобы белое было видно на светлом фоне */
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
  }

  .track-thumb {
    position: absolute;
    width: 16px;
    height: 16px;
    background-color: rgba(61, 93, 236, 1);
    border-radius: 50%;
    transform: translate(-50%, 0); /* Центрируем относительно точки left */
    transition:
      transform 0.1s ease,
      box-shadow 0.1s ease;
    z-index: 2;
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.2);
  }

  .track-thumb:hover,
  .track-thumb.active {
    transform: translate(-50%, 0) scale(1.2);
    box-shadow: 0 0 8px rgba(61, 93, 236, 0.5);
  }
</style>
