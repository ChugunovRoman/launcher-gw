---
feature: folder-patch-mode
status: delivered
updated: 2026-10-21
branch: feat/folder-patch-mode
commits: 23e2c82..HEAD
---

# Folder patch collect mode

## Report

**What was built** — `collect_patch` now has two automatic modes. When any git repository exists under the selected source folder, collection is unchanged (tag→HEAD diffs of every nested repo). When `find_git_roots` is empty, the collector switches to **folder mode** and copies every file under the folder into the patch tree (relative paths preserved), applying the same exclude globs and save-breaking markers. Folder mode reports `collect_mode: "folder"`, empty `deleted_files`/`base_tag`/FE fragment, and one synthetic repo report. The Releases collect form shows a live mode badge via `detect_patch_source_mode`. Folder-mode uploads send `gameSourceDir: null` so tagging is skipped; if a non-repo path is still supplied, TagRepos is marked skipped with a dedicated `NO_GAME_REPOS` hint (not the misleading `NO_GAME_SOURCE_DIR`).

**Verification** — `cargo test patch_collect` 9/9 PASS (mode probe, folder copy + excludes + `.git` skip, save-breaking, empty-after-excludes, prepared `patch/0.5.5-patch5` E2E); `cargo test handlers::patches` 5/5 PASS (includes `empty_tag_repos_uses_distinct_skip_code`); `svelte-check` 0 errors / 81 pre-existing warnings.

**Journey log**
- Worktree needed `src-tauri/icons/*` copied from the main checkout — `tauri-build` fails without `icon.ico` (not in git).
- `npm run check` is broken in this repo (`svelte-kit` is not a dependency); run `node_modules/.bin/svelte-check` directly.
- Review caught reusing `SKIP_NO_GAME_SOURCE_DIR` for the empty-repos case — that code maps to a “folder is not set” hint and contradicted the real cause; split out `NO_GAME_REPOS`.
- The prepared-tree E2E test used `.expect` on `canonicalize` while claiming to skip when absent — now `let Ok(...) else { return }`.
- First PowerShell `git show … > file` export wrote UTF-16; always use `cmd /c` redirection for raw blobs.

## [S1] Problem

`collect_patch` in the launcher only builds a patch from **git history**: it walks repositories under the selected folder, diffs the latest reachable tag against HEAD, and copies committed changes.

Developers also need a second path: take a **prepared folder** (for example `GW/patch/0.5.5-patch5/` with an already GlobalWar-relative tree like `gamedata/scripts/...`) and pack **every file in it** into a patch, with no repository, no tags, and no commit range.

Today that folder is not a git checkout, so `find_git_roots` returns nothing, `collect_patch` produces an empty patch (`changed: 0`, no `base_tag`), and the collect UI looks broken or useless. The comment in `src/Views/Releases.svelte` (“Элемент сбора патча из git-репозиториев игры”) marks this section as git-only.

The folder mode must engage **automatically** when the selected path has no git repository — no manual mode switch.

## [S2] Design

### Mode selection (automatic)

At the start of `collect_patch` (`src-tauri/src/utils/patch_collect.rs`):

1. Run `find_git_roots(&source_dir)` (already exists; finds `.git` dirs/files, skips descending into `.git`).
2. If the list is **non-empty** → **git mode** (current behavior, unchanged).
3. If the list is **empty** → **folder mode** (new).

Rationale: the user wording is “по указанному пути нет гит репозитория”. The existing collector already treats nested repos/submodules under the selected path as first-class sources; switching only when *no* repo is found preserves that and matches the prepared-folder case (`patch/0.5.5-patch5` has neither root nor nested `.git`).

No UI toggle. The result reports the mode so the UI can label it.

### Collect result contract

Extend `PatchCollectResult` (Rust + `src/typing.d.ts`) with:

```ts
collect_mode: "git" | "folder"
```

Folder-mode fields:

| Field | Value | Why |
|---|---|---|
| `patch_dir` | temp `patches/gw-patch-<uuid>` | same staging as git mode |
| `deleted_files` | `[]` | no baseline to diff deletions |
| `base_tag` | `null` | no git tags |
| `repos` | one synthetic `RepoPatchReport` | keeps the existing report UI working |
| `fe_updated_fields` / `fe_fragment_entries` | empty / 0 | no git blobs to diff `faction_editor_default_config.ltx` |
| `breaks_saves` / `save_breaking_files` | still computed from copied paths | same markers as git mode |

Synthetic report:

- `repo_rel_path`: `""` (root)
- `base_tag`: `""`
- `status`: `"collected"` if anything was copied, else `"no_changes"`
- `changed` / `deleted`: file counts
- `message`: `None`, or a short note when the folder is empty / fully excluded

### Folder collect algorithm

`collect_folder_patch(...)` in `patch_collect.rs`, called from `collect_patch` instead of the per-repo loop:

1. Reuse `build_exclude_set` and `build_save_breaking_set`.
2. `walkdir` over `source_dir` with `filter_entry` skipping `.git` (same as `find_git_roots`).
3. For each **file** (not directory):
   - `rel` = path relative to `source_dir`, normalized with `to_rel_slash`.
   - Skip if `exclude_set` matches `rel` (same semantics as `collect_repo`).
   - `note_save_breaking(...)` on `rel`.
   - `fs::copy` into `patch_dir.join(rel)`, creating parents.
   - Count and emit `CollectStage::Copy` progress (throttled as today).
4. Symlinks: walkdir with `follow_links(false)`; copy only if `file_type().is_file()`.
5. Progress stages: `Scan` → `Copy` (throttled) → `Done`. `PatchCollectProgress.repo` = source folder name. `repos_total` = 1.
6. If zero files survive excludes: `Ok` with `changed: 0`, `status: no_changes`, and a note — do **not** error (same as a git collect with no deltas).

Exclude patterns stay active in folder mode: a prepared patch tree must not accidentally ship `*.pdb`, `*.txt`, `.git`, `utils/**` if the developer points at a broader folder.

### Upload stage implications (`handlers/patches.rs`)

`upload_patch` accepts `game_source_dir: Option<String>` and tags repos at the end (`Stage::TagRepos`).

For a folder-mode patch there is nothing to tag:

- Frontend: when `patchResult.collect_mode === "folder"`, send `gameSourceDir: null` on `upload_patch`. Tag stage then takes the existing `None` branch (`SKIP_NO_GAME_SOURCE_DIR`).
- Backend: when `tag_game_repos` returns an empty `repos` vec (path set but no git roots), mark `Stage::TagRepos` as `skipped` with **`SKIP_NO_GAME_REPOS`** (`NO_GAME_REPOS`) and the matching `hint.noGameRepos` string — not `SKIP_NO_GAME_SOURCE_DIR`, whose hint claims the folder is unset.

`deleted_files` / `base_release_tag` already tolerate empty/null. FE fragment finalize is a no-op when nothing was staged.

### UI (`src/Views/Releases.svelte` + locales)

Under the source path row, show the automatic mode badge (no control):

- `app.releases.patch.mode.git` — “Git repositories (latest tag → HEAD)”
- `app.releases.patch.mode.folder` — “All files in folder (no git)”

Detection for the badge: `detect_patch_source_mode(source_dir: String) -> "git" | "folder"` (wrapper around `find_git_roots(...).is_empty()`), invoked when the path changes (browse or restored from config) and reused from `collect_mode` after collect.

Result summary includes the mode label. i18n keys in `src/locales/en.json` and `src/locales/ru.json` only.

### Progress events

Keep `PatchCollectProgress` shape. Folder mode uses `stage: "copy"` with `repo: "<folder name>"`, `repos_total: 1`.

### Tests

1. **Mode probe**: temp dir without `.git` → folder; with a nested `.git` dir → git.
2. **Folder collect copies tree**: nested files under `gamedata/scripts/...` land in `patch_dir` with the same relative paths; `.git` and an excluded `*.pdb` do not.
3. **Folder collect save-breaking**: a copied `gamedata/spawns/all.spawn` sets `breaks_saves`.
4. **Folder collect empty after excludes**: `changed == 0`, `collect_mode == "folder"`, `base_tag.is_none()`, `deleted_files.is_empty()`.
5. **Prepared-tree E2E**: `patch/0.5.5-patch5` when present — folder mode + `gamedata/scripts/ui_mm_opt_main.script` copied; skipped when the fixture is absent.
6. **TagRepos skip code**: `empty_tag_repos_skip()` returns `NO_GAME_REPOS`, distinct from `NO_GAME_SOURCE_DIR`.

## [S3] Out of Scope

- Manual mode switch or “force folder mode when git exists”.
- Deleting files in folder mode (no baseline).
- Generating a faction-editor settings fragment without git blobs (developer can drop a ready `appdata/patches/*.faction_editor_patch.ltx` into the folder; it ships as a normal file).
- Changing pack/upload/index pipelines beyond the TagRepos skip hardening.
- Collecting from a folder that is a partial GlobalWar tree but *also* a git checkout (that stays git mode).
- CLI or Pack-tab integration.

## Tasks

- [x] T1: Add `CollectMode` + `collect_mode` on `PatchCollectResult`; branch `collect_patch` into existing git loop vs new `collect_folder_patch` — acceptance: `cargo test patch_collect` covers mode probe + folder copy (covers: S2)
- [x] T2: Implement `collect_folder_patch` (walk, excludes, copy, save-breaking, Scan/Copy/Done progress) — acceptance: nested `gamedata/...` files copied; excludes and `.git` skipped; `breaks_saves` works (covers: S2; depends: T1)
- [x] T3: Expose `detect_patch_source_mode` command and register it — acceptance: invoke returns `"git"`/`"folder"` for fixture dirs (covers: S2; depends: T1)
- [x] T4: TagRepos hardening — empty `tag_game_repos` result marks stage skipped with `NO_GAME_REPOS` — acceptance: unit test pins the distinct skip code (covers: S2)
- [x] T5: Frontend — `collect_mode` in `typing.d.ts`, mode badge on source path, result label, `gameSourceDir: null` on folder-mode upload, en/ru strings — acceptance: `svelte-check` 0 errors; badge updates when path has no `.git` (covers: S2; depends: T1, T3)
- [x] T6: End-to-end sanity on a real prepared tree (`patch/0.5.5-patch5`) — acceptance: collect reports `folder`, copies `gamedata/scripts/ui_mm_opt_main.script`, patch_dir layout is GlobalWar-relative (covers: S2; depends: T2, T5)
