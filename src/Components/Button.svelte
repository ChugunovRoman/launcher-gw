<script lang="ts">
  let { isDisabled = $bindable(false), isRed = $bindable(false), onclick } = $props();
</script>

<span role="button" tabindex="0" class="btn" class:btn_inactive={isDisabled} class:btn_red={isRed} {onclick}>
  <slot />
</span>

<style>
  .btn {
    padding: 10px 40px;
    color: white;
    background-color: rgba(61, 93, 236, 0.8);
    transition: background-color 0.15s ease;
    border-radius: 3px;
    -webkit-app-region: no-drag;
  }
  .btn:hover {
    cursor: pointer;
    background-color: rgba(61, 93, 236, 1);
  }

  .btn_inactive {
    cursor: default;
    background-color: rgba(0, 0, 0, 0.8);
  }
  .btn_inactive:hover {
    background-color: rgba(0, 0, 0, 0.8);
  }

  .btn_red {
    background-color: rgba(251, 50, 0, 0.8);
  }
  .btn_red:hover {
    background-color: rgba(251, 50, 0, 1);
  }
</style>
