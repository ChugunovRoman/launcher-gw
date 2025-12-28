<script lang="ts">
  import { fly, fade } from "svelte/transition";
  import { X } from "lucide-svelte";

  let {
    showModal = $bindable(false),
    header,
    children,
    footer,
    onClose, // Замена dispatch
  } = $props();

  let dialog: HTMLDialogElement;

  $effect(() => {
    if (showModal) {
      dialog?.showModal();
    } else {
      dialog?.close();
    }
  });

  function close() {
    showModal = false;
    if (onClose) onClose();
  }
</script>

{#if showModal}
  <dialog bind:this={dialog} onclose={close} onclick={(e) => e.target === dialog && close()} transition:fade={{ duration: 200 }}>
    <div class="modal-content" transition:fly={{ y: -50, duration: 300 }}>
      <header>
        {#if header}
          {@render header()}
        {/if}
        <div role="button" onclick={close} class="close-btn"><X size={16} /></div>
      </header>

      <main>
        {@render children?.()}
      </main>

      {#if footer}
        <footer>
          {@render footer()}
        </footer>
      {/if}
    </div>
  </dialog>
{/if}

<style>
  dialog {
    padding: 0;
    border: none;
    border-radius: 12px;
    background: transparent;
    max-width: 500px;
    width: 90%;
  }

  dialog::backdrop {
    background: rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(2px);
  }

  .modal-content {
    background: rgba(46, 39, 63, 0.8);
    padding: 20px;
    border-radius: 12px;
    box-shadow: 0 15px 35px rgba(0, 0, 0, 0.2);
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 15px;
    font-weight: bold;
  }

  .close-btn {
    background: none;
    border: none;
    font-size: 1.5rem;
    cursor: pointer;
    color: white;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 15px;
  }
</style>
