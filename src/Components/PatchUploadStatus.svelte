<!-- Stage stepper + live detail of an upload (patch now, full release later).
     Stage names and code hints come from the `stageLabelKeys` / `hintKeys`
     maps, so the markup has no knowledge of a particular upload kind.
     plans/launcher/patch-upload-status-plan.md, stage 2.2. -->
<script lang="ts">
  import { _ } from "svelte-i18n";
  import Progress from "./Progress.svelte";
  import Spin from "./Spin.svelte";
  import { parseBytes } from "../utils/dwn";
  import { isPatchUploadActive, type PatchUploadState } from "../store/patchUpload";

  let {
    state,
    stageOrder,
    stageLabelKeys,
    hintKeys = {},
    doneKey = "app.releases.patch.uploaded",
    uploadStage = "upload",
    packStage = "packing",
    tagStage = "tag_repos",
  }: {
    state: PatchUploadState;
    stageOrder: readonly string[];
    stageLabelKeys: Record<string, string>;
    hintKeys?: Record<string, string>;
    doneKey?: string;
    uploadStage?: string;
    packStage?: string;
    tagStage?: string;
  } = $props();

  const STATE_ICONS: Record<string, string> = {
    pending: "○",
    done: "✓",
    skipped: "–",
    warning: "!",
    failed: "✗",
    running: "●",
  };

  const PACK_STATUS_KEYS: Record<number, string> = {
    0: "app.pack.addFiles",
    1: "app.pack.compressing",
    2: "app.pack.hashing",
  };

  const FILE_STATUS_KEYS: Record<string, string> = {
    uploading: "app.releases.patch.fileStatus.uploading",
    waiting_server: "app.releases.patch.fileStatus.waitingServer",
    verifying: "app.releases.patch.fileStatus.verifying",
    retrying: "app.releases.patch.fileStatus.retrying",
    done: "app.releases.patch.fileStatus.done",
  };

  const active = $derived(isPatchUploadActive(state));
  const hasAnything = $derived(state.status !== "idle" || Object.keys(state.stages).length > 0);
  const overallProgress = $derived(state.totalSize > 0 ? (state.totalUploaded / state.totalSize) * 100 : 0);
  const lastLog = $derived(state.log.length > 0 ? state.log[state.log.length - 1] : "");

  function stageState(stage: string): string {
    return (state.stages as Record<string, { state: string } | undefined>)[stage]?.state ?? "pending";
  }

  function stageInfo(stage: string): { state: string; message?: string; code?: string } | undefined {
    return (state.stages as Record<string, { state: string; message?: string; code?: string } | undefined>)[stage];
  }

  /// The current step keeps its spinner while the upload runs, even after an
  /// intermediate warning of that step.
  function isSpinning(stage: string): boolean {
    if (!active || state.currentStage !== stage) return false;
    const st = stageState(stage);
    return st === "running" || st === "warning";
  }

  function stageLabel(stage: string | null): string {
    if (!stage) return "";
    const key = stageLabelKeys[stage];
    return key ? $_(key) : stage;
  }

  function hintFor(code: string | null | undefined): string {
    if (!code) return "";
    const key = hintKeys[code];
    return key ? $_(key) : "";
  }

  function bytes(value: number): string {
    const [v, sfx] = parseBytes(value);
    return `${v} ${$_(`app.common.${sfx}`)}`;
  }

  function speed(data: UploadFileData): string {
    if (!data.speedValue || !data.sfxValue) return "";
    return $_("app.releases.patch.speed", { values: { value: data.speedValue, unit: $_(`app.common.${data.sfxValue}`) } });
  }

  function fileStatusText(name: string): string {
    const st = state.fileStatus.get(name);
    if (!st || st === "uploading") return "";
    const key = FILE_STATUS_KEYS[st];
    return key ? $_(key) : "";
  }

  function stageDone(stage: string): boolean {
    const st = stageState(stage);
    return st !== "pending";
  }
</script>

{#if hasAnything}
  <div class="upload-status">
    <div class="stepper">
      {#each stageOrder as stage}
        {@const st = stageState(stage)}
        {@const info = stageInfo(stage)}
        <div class="step" class:current={active && state.currentStage === stage}>
          <span class="step-icon {st}" title={$_(`app.releases.patch.stageState.${st}`)}>
            {#if isSpinning(stage)}
              <Spin size={12} />
            {:else}
              {STATE_ICONS[st] ?? STATE_ICONS.pending}
            {/if}
          </span>
          <span class="step-label">{stageLabel(stage)}</span>
          {#if stage === uploadStage && state.filesTotal > 0}
            <span class="step-extra">{state.filesDone} / {state.filesTotal}</span>
          {/if}
          {#if info?.message && st !== "running" && st !== "failed"}
            <span class="step-message" class:warn={st === "warning"}>{info.message}</span>
          {/if}
        </div>
        {#if st === "warning" && hintFor(info?.code)}
          <div class="step-detail step-hint">{hintFor(info?.code)}</div>
        {/if}

        <!-- Packing: status, current file, progress, sizes. -->
        {#if stage === packStage && active && state.currentStage === packStage && state.pack}
          <div class="step-detail">
            <div class="detail-row">
              <span>{PACK_STATUS_KEYS[state.pack.status] ? $_(PACK_STATUS_KEYS[state.pack.status]) : ""}</span>
              <span class="detail-muted">{bytes(state.pack.processed_size)} / {bytes(state.pack.total_size)}</span>
            </div>
            <Progress height={12} progress={state.pack.percentage} showPercents={false} />
            {#if state.pack.current_file}
              <span class="detail-file">{state.pack.current_file}</span>
            {/if}
          </div>
        {/if}

        <!-- Upload: overall progress, files counter, per-file rows. -->
        {#if stage === uploadStage && stageDone(uploadStage) && state.files.size > 0}
          <div class="step-detail">
            <div class="detail-row">
              <span>{$_("app.releases.patch.overall")}</span>
              <span class="detail-muted">
                {$_("app.releases.patch.filesCount", { values: { done: state.filesDone, total: state.filesTotal } })}
                · {bytes(state.totalUploaded)} / {bytes(state.totalSize)}
              </span>
            </div>
            <Progress height={12} progress={overallProgress} showPercents={false} />
            {#each [...state.files] as [name, file] (name)}
              <div class="file-line">
                <span class="file-name" title={name}>{name}</span>
                <Progress height={8} progress={file.progress} showPercents={false} />
                <span class="file-size">{bytes(file.file_uploaded_size)} / {bytes(file.file_total_size)}</span>
                <span class="file-speed">{active ? speed(file) : ""}</span>
                <span class="file-status" class:ok={state.fileStatus.get(name) === "done"}
                  >{state.fileStatus.get(name) === "done" ? STATE_ICONS.done : fileStatusText(name)}</span>
              </div>
            {/each}
          </div>
        {/if}

        <!-- Tags: one row per processed repo, as they come. -->
        {#if stage === tagStage && state.repos.length > 0}
          <div class="step-detail">
            {#each state.repos as repo (repo.repo_rel_path)}
              <div class="repo-line">
                <span class="repo-path">{repo.repo_rel_path || "(root)"}</span>
                <span class={repo.pushed ? "repo-ok" : "repo-err"}>
                  {#if repo.pushed}
                    {$_("app.releases.patch.tagOk")}
                  {:else}
                    {$_("app.releases.patch.tagFail")}{repo.message ? `: ${repo.message}` : ""}
                  {/if}
                </span>
              </div>
            {/each}
          </div>
        {/if}
      {/each}
    </div>

    {#if active && lastLog}
      <span class="last-log">{lastLog}</span>
    {/if}

    {#if state.status === "cancelling"}
      <div class="terminal muted">{$_("app.releases.patch.cancelling")}</div>
    {:else if state.status === "done"}
      <div class="terminal ok">
        <span class="terminal-icon">✓</span>
        <span>{$_(doneKey)}</span>
      </div>
    {:else if state.status === "failed" && state.error}
      <div class="terminal err">
        <div>
          <span class="terminal-icon">✗</span>
          {#if state.error.stage}
            {$_("app.releases.patch.failedAt", { values: { stage: stageLabel(state.error.stage) } })}
          {:else}
            {$_("app.releases.patch.failedUnknown")}
          {/if}
          {state.error.message ? `: ${state.error.message}` : ""}
        </div>
        {#if hintFor(state.error.code)}
          <div class="terminal-hint">{hintFor(state.error.code)}</div>
        {/if}
      </div>
    {:else if state.status === "cancelled"}
      <div class="terminal muted">{$_("app.releases.patch.cancelled")}</div>
    {/if}

    {#if state.warnings.length > 0}
      <div class="warnings">
        <span class="warnings-title">{$_("app.releases.patch.warningsTitle")}</span>
        {#each state.warnings as w}
          <span class="warning-line">{w}</span>
        {/each}
      </div>
    {/if}

    {#if state.log.length > 0}
      <details class="log-details">
        <summary>{$_("app.releases.patch.showLog")}</summary>
        {#each state.log as text}
          <span class="log-text">{text}</span>
        {/each}
      </details>
    {/if}
  </div>
{/if}

<style>
  .upload-status {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .stepper {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  .step {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
    color: #aaa;
  }
  .step.current {
    color: #fff;
  }

  .step-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    flex-shrink: 0;
    font-weight: bold;
  }
  .step-icon.pending {
    color: #777;
  }
  .step-icon.done {
    color: #4caf50;
  }
  .step-icon.skipped {
    color: #999;
  }
  .step-icon.warning {
    color: #ffca28;
  }
  .step-icon.failed {
    color: #f44336;
  }

  .step-label {
    font-weight: 500;
  }

  .step-extra {
    color: #ddd;
    font-family: monospace;
    font-size: 0.8rem;
  }

  .step-message {
    color: #999;
    font-size: 0.8rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .step-message.warn {
    color: #ffca28;
  }

  .step-detail {
    margin: 0.2rem 0 0.4rem 1.5rem;
    color: #ddd;
    font-size: 0.8rem;
  }

  .step-hint {
    color: #ffca28;
  }

  .detail-row {
    display: flex;
    justify-content: space-between;
    gap: 0.75rem;
  }

  .detail-muted {
    color: #aaa;
    font-family: monospace;
  }

  .detail-file {
    display: block;
    color: #aaa;
    font-family: monospace;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .file-line {
    display: grid;
    grid-template-columns: 140px 1fr 150px 90px 170px;
    align-items: center;
    gap: 0.5rem;
  }

  .file-name {
    color: #ddd;
    font-family: monospace;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .file-size,
  .file-speed {
    justify-self: end;
    color: #aaa;
    font-family: monospace;
  }

  .file-status {
    color: #aaa;
    font-style: italic;
  }
  .file-status.ok {
    color: #4caf50;
    font-style: normal;
  }

  .repo-line {
    display: grid;
    grid-template-columns: 220px 1fr;
    gap: 0.75rem;
    margin-bottom: 0.25rem;
  }

  .repo-path {
    color: #aaa;
    font-family: monospace;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .repo-ok {
    color: #4caf50;
  }
  .repo-err {
    color: #f44336;
  }

  .last-log {
    color: #999;
    font-family: monospace;
    font-size: 0.75rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .terminal {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    margin-top: 0.5rem;
    font-weight: 500;
  }
  .terminal.ok {
    flex-direction: row;
    align-items: center;
    gap: 0.5rem;
    color: #4caf50;
  }
  .terminal.err {
    color: #f44336;
  }
  .terminal.muted {
    color: #999;
  }

  .terminal-icon {
    font-size: 1.1rem;
  }

  .terminal-hint {
    color: #ff8a80;
    font-weight: normal;
    font-size: 0.85rem;
  }

  .warnings {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    padding: 0.5rem 0.75rem;
    border: 1px solid rgba(255, 193, 7, 0.6);
    border-radius: 6px;
    font-size: 0.85rem;
  }

  .warnings-title {
    color: #ffca28;
    font-weight: 500;
  }

  .warning-line {
    color: #ffe082;
    white-space: pre-wrap;
  }

  .log-details summary {
    color: #aaa;
    cursor: pointer;
    font-size: 0.85rem;
  }

  .log-text {
    display: block;
    text-align: left;
    font-size: 0.8rem;
    color: #aaa;
    font-family: monospace;
  }
</style>
