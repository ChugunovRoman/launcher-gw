<script lang="ts">
  import { _ } from "svelte-i18n";
  import { marked } from "marked";
  import DOMPurify from "dompurify";
  import { showDlgPatchNotes, patchNotesData } from "../store/main";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";

  /// Converts GitHub blob URLs to raw URLs so <img> tags get actual image data
  /// instead of an HTML page.  e.g.:
  ///   github.com/user/repo/blob/branch/path/img.png
  ///   → raw.githubusercontent.com/user/repo/branch/path/img.png
  function fixImageUrls(md: string): string {
    return md.replace(
      /https?:\/\/github\.com\/([^/]+)\/([^/]+)\/blob\/([^)\s"']+)/g,
      (_, user, repo, rest) => `https://raw.githubusercontent.com/${user}/${repo}/${rest}`,
    );
  }

  const renderedHtml = $derived(
    $patchNotesData?.notes
      ? DOMPurify.sanitize(
          marked.parse(fixImageUrls($patchNotesData.notes), { gfm: true, breaks: true, async: false }) as string,
        )
      : "",
  );
</script>

<Modal bind:showModal={$showDlgPatchNotes} maxWidth="90%">
  {#snippet header()}
    <span class="modal-title">{$_("app.patches.notes")}</span>
  {/snippet}

  <div class="patch-notes-body">
    {#if $patchNotesData}
      <h3 class="patch-name-title">{$patchNotesData.title}</h3>
      {#if renderedHtml}
        <div class="markdown-body">{@html renderedHtml}</div>
      {:else}
        <p class="no-notes">{$_("app.patches.noNotes")}</p>
      {/if}
    {/if}
  </div>

  {#snippet footer()}
    <Button onclick={() => ($showDlgPatchNotes = false)}>OK</Button>
  {/snippet}
</Modal>

<style>
  .modal-title {
    color: #fff;
    font-size: 1.3rem;
    flex: 1;
    text-align: center;
  }
  .patch-notes-body {
    min-height: 60vh;
    max-height: 70vh;
    overflow-y: auto;
    text-align: left;
  }
  .patch-notes-body::-webkit-scrollbar {
    width: 12px;
  }
  .patch-notes-body::-webkit-scrollbar-track {
    background: transparent;
  }
  .patch-notes-body::-webkit-scrollbar-thumb {
    background-color: rgba(61, 93, 236, 0.8);
    border-radius: 6px;
    border: 3px solid transparent;
    background-clip: content-box;
  }
  .patch-notes-body::-webkit-scrollbar-thumb:hover {
    background-color: rgba(61, 93, 236, 1);
  }
  .patch-notes-body::-webkit-scrollbar-button {
    display: none;
  }
  .patch-name-title {
    color: #4caf50;
    margin: 0 0 1rem;
    font-size: 1.1rem;
    text-align: left;
  }
  .no-notes {
    color: #999;
    font-style: italic;
    margin: 0;
    text-align: left;
  }

  /* Markdown rendered content (dark theme) */
  .markdown-body {
    color: #ddd;
    font-family: system-ui, sans-serif;
    font-size: 0.9rem;
    line-height: 1.6;
    text-align: left;
  }
  .markdown-body :global(h1),
  .markdown-body :global(h2),
  .markdown-body :global(h3),
  .markdown-body :global(h4) {
    color: #fff;
    margin-top: 1.2rem;
    margin-bottom: 0.5rem;
  }
  .markdown-body :global(h1) { font-size: 1.4rem; }
  .markdown-body :global(h2) { font-size: 1.2rem; }
  .markdown-body :global(h3) { font-size: 1.05rem; }
  .markdown-body :global(h4) { font-size: 0.95rem; }

  .markdown-body :global(p) {
    margin: 0.5rem 0;
  }

  .markdown-body :global(ul),
  .markdown-body :global(ol) {
    padding-left: 1.5rem;
    margin: 0.5rem 0;
  }
  .markdown-body :global(li) {
    margin: 0.25rem 0;
  }

  .markdown-body :global(code) {
    font-family: Consolas, Monaco, "Courier New", monospace;
    background: rgba(0, 0, 0, 0.35);
    padding: 0.15em 0.4em;
    border-radius: 3px;
    font-size: 0.85em;
  }
  .markdown-body :global(pre) {
    background: rgba(0, 0, 0, 0.35);
    padding: 0.75rem 1rem;
    border-radius: 6px;
    overflow-x: auto;
    margin: 0.75rem 0;
  }
  .markdown-body :global(pre code) {
    background: none;
    padding: 0;
    border-radius: 0;
  }

  .markdown-body :global(a) {
    color: #6db3f2;
    text-decoration: underline;
  }
  .markdown-body :global(a:hover) {
    color: #90c8ff;
  }

  .markdown-body :global(blockquote) {
    border-left: 3px solid rgba(255, 255, 255, 0.3);
    margin: 0.75rem 0;
    padding: 0.25rem 1rem;
    color: #aaa;
    font-style: italic;
  }

  .markdown-body :global(hr) {
    border: none;
    border-top: 1px solid rgba(255, 255, 255, 0.15);
    margin: 1rem 0;
  }

  .markdown-body :global(strong) {
    color: #fff;
  }

  .markdown-body :global(table) {
    border-collapse: collapse;
    margin: 0.75rem 0;
    width: 100%;
  }
  .markdown-body :global(th),
  .markdown-body :global(td) {
    border: 1px solid rgba(255, 255, 255, 0.15);
    padding: 0.4rem 0.75rem;
    text-align: left;
  }
  .markdown-body :global(th) {
    color: #fff;
    background: rgba(255, 255, 255, 0.05);
  }
</style>
