---
feature: fps-limit
status: in-progress
updated: 2026-10-25
branch: master
commits: 
---

# FPS limit setting (rs_fps_limit)

## Report

## [S1] Problem

The game's frame limiter is the engine cvar `rs_fps_limit`
(`xray-16/src/xrEngine/xr_ioc_cmd.cpp:896`, `CMD4(CCC_Integer, "rs_fps_limit", &ps_fps_limit, 30, 501)`).
The launcher's run-params view (`src/Views/RunParams.svelte`) exposes resolution,
renderer, V-Sync and FOV, but not this cvar — players have to use the in-game
options trackbar or the console.

The setting must be a combobox with exactly these entries, in this order:
180, 120 (default), 60, 30, "Без ограничения" / "Unlimited". It persists like the
other run params and lands in `user.ltx` / `tmp.ltx` on save and before launch.

## [S2] Design

### Value contract

`rs_fps_limit` is an integer cvar in **[30, 501]**. `CCC_Integer::Execute` rejects
out-of-range values instead of clamping (`xray-16/src/xrEngine/xr_ioc_cmd.h:583-585`),
so 0 / -1 are NOT "unlimited" — the line is dropped with a syntax error and the
previous value stays. The limiter computes `updateDelta = 1000 / ps_fps_limit` and
sleeps the remainder (`xray-16/src/xrEngine/device.cpp:737-757`); the engine default
is `ps_fps_limit = 501` (`device.cpp:55`), i.e. a 1 ms frame budget ≈ uncapped.
501 is therefore the "no limit" sentinel (it is also the cvar's max and the in-game
trackbar's max):

| Combobox entry | Value written to ltx |
|---|---|
| 180 | 180 |
| 120 (default) | 120 |
| 60 | 60 |
| 30 | 30 |
| Без ограничения / Unlimited | **501** |

Dropdown order is exactly 180, 120, 60, 30, Unlimited.

### Data model

`RunParams.rs_fps_limit: u32` in `src-tauri/src/configs/AppConfig.rs` with
`#[serde(default = "default_rs_fps_limit")]` (returns 120) and `impl Default` → 120.
The dedicated default fn is required (same as the alife fields): plain
`#[serde(default)]` would give 0, which the engine rejects. A config.json saved
before this feature gets 120 on load.

TS mirror: `rs_fps_limit: number` in `src/typing.d.ts` (`RunParams`).

Shared constants in `src/consts.ts`:

```ts
export const FPS_LIMIT_UNLIMITED = 501;
export const FPS_LIMIT_DEFAULT = 120;
export const FPS_LIMIT_OPTIONS = [180, 120, 60, 30, FPS_LIMIT_UNLIMITED];
```

### user.ltx write

In `apply_run_params_to_ltx` (`src-tauri/src/handlers/user_ltx.rs:222-268`), next to
the other `rs_*` keys:

```rust
ltx.set("rs_fps_limit".to_string(), run_params.rs_fps_limit.to_string());
```

`GameConfig` appends missing keys, so both `user.ltx` and `tmp.ltx` (the fn is
applied to both on save and in `prepare_ltx_for_launch`) gain the key. The raw
value is written (like `fov`); no write-time normalization — see [S3].

Preset collision: `apply_selected_preset` runs before the explicit keys
(`user_ltx.rs:234` vs `:245+`), so run_params win over a preset that also carries
`rs_fps_limit` — same precedence as `rs_v_sync` today.

### UI (RunParams.svelte)

A select row in the left options column right after the renderer row (display
settings group). State `selectedFpsLimit: number`, initialized to
`FPS_LIMIT_DEFAULT`. The Unlimited entry is bound to `FPS_LIMIT_UNLIMITED` and
labeled via i18n. Save maps `rs_fps_limit: selectedFpsLimit` into the `RunParams`
payload.

Load fallback (same pattern as the vid_mode fix, `RunParams.svelte:196-200`): a
saved value outside `FPS_LIMIT_OPTIONS` displays as `FPS_LIMIT_DEFAULT`, so the
select is never blank and the next save writes a valid value.

### i18n

`app.params` in `src/locales/ru.json` / `src/locales/en.json` (naming follows
`showFps: "Отображение ФПС (rs_fps)"`):

| key | ru | en |
|---|---|---|
| `fpsLimit` | Лимит ФПС (rs_fps_limit): | FPS limit (rs_fps_limit): |
| `fpsLimitUnlimited` | Без ограничения | Unlimited |

## [S3] Out of Scope

- `rs_fps_limit_in_menu` (menu limiter, engine default 60) stays engine-side.
- The in-game options trackbar (`ui_mm_opt.xml`) is untouched.
- No write-time clamping of hand-edited config values (raw u32, like `fov`).
- Free-form values (e.g. 144) are not offered in the combobox; a config holding
  one shows as 120 and is overwritten on the next save (vid_mode precedent).

## Tasks

- [ ] T1: Backend `rs_fps_limit` field + ltx write + unit test — acceptance: `RunParams` carries `rs_fps_limit` (default 120, serde default 120); `apply_run_params_to_ltx` writes `rs_fps_limit 501` for the unlimited value and `rs_fps_limit 120` for a default config into a temp ltx; `cargo test` in `src-tauri` green. (covers: S2 Data model, S2 user.ltx write)
- [ ] T2: TS type + shared constants — acceptance: `RunParams` in `src/typing.d.ts` declares `rs_fps_limit: number`; `FPS_LIMIT_UNLIMITED` / `FPS_LIMIT_DEFAULT` / `FPS_LIMIT_OPTIONS` exist in `src/consts.ts` with the documented values. (covers: S2 Data model)
- [ ] T3: i18n labels — acceptance: `app.params.fpsLimit` and `app.params.fpsLimitUnlimited` exist in both locale files with the documented text. (covers: S2 i18n)
- [ ] T4: Combobox in RunParams.svelte — acceptance: the view shows a select right after the renderer row with the five entries in the specified order, defaults to 120, falls back to 120 for unknown saved values, and sends the chosen number as `rs_fps_limit` on save. (covers: S2 UI; depends: T2, T3)
- [ ] T5: Verification — acceptance: `cargo test` (src-tauri) and `svelte-check` run clean (0 errors; warnings pre-existing) with results recorded. (covers: S2; depends: T1, T4)
