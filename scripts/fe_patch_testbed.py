#!/usr/bin/env python3
"""Local test bed for "faction editor settings carried by a patch".

Lets you exercise the whole player-side path — the post-install dialog, the
"Применить настройки" button, the targeted config edit, the backup and the
marker update — WITHOUT uploading or publishing anything, so players on an
older launcher can never see a half-finished patch.

What it does: drops a settings fragment next to an ALREADY INSTALLED patch in
`<install>/appdata/patches/` and tags that patch's marker with the props the
fragment carries. That is byte-for-byte the state a real patch install leaves
behind, so from there the launcher runs its real code.

It deliberately reuses an existing installed patch instead of inventing one:
a made-up marker would break the patch chain check in `start_install_patch`
(`base_patch` vs last installed).

Spec: plans/launcher/faction-editor-patch-fields-plan.md

Usage (from launcher-new/):
    python scripts/fe_patch_testbed.py show
    python scripts/fe_patch_testbed.py stage
    python scripts/fe_patch_testbed.py stage --full-diff --old-rev 0.5.2-Beta
    python scripts/fe_patch_testbed.py clean

`--install` defaults to the newest install found in the launcher's config.json.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

FRAGMENT_SUFFIX = ".faction_editor_patch.ltx"
CONFIG_LTX = "faction_editor_config.ltx"
WRITE_LTX = "faction_editor_config.write.ltx"
DEFAULT_CONFIG_LTX = "faction_editor_default_config.ltx"

# Mirrors FE_PATCHABLE_FIELDS in src-tauri/src/consts.rs. Anything outside this
# list is rejected by the launcher, so the test bed must not emit it either.
PATCHABLE_FIELDS = [
    "power",
    "power_leader",
    "leader_min_money",
    "leader_max_money",
    "leader_min_reputation",
    "leader_max_reputation",
    "fire_wound_immunity_leader",
    "explosion_immunity_leader",
    "fire_wound_preset",
    "explosion_preset",
    "mutant_alliance",
    "descr_diff",
    "spot_color_r",
    "spot_color_g",
    "spot_color_b",
    "min_money",
    "max_money",
    "min_reputation",
    "max_reputation",
    "fire_wound_immunity",
    "explosion_immunity",
]

VISUALS_MARKER = "_visuals_"
# Mirrors is_valid_section_name in src-tauri/src/service/faction_patch.rs.
SECTION_RE = re.compile(r"^[a-z0-9_]{1,64}$")


# --------------------------------------------------------------------------
# Locating things
# --------------------------------------------------------------------------

def launcher_config_path() -> Path:
    appdata = os.environ.get("APPDATA")
    if not appdata:
        sys.exit("APPDATA is not set — pass --install explicitly.")
    return Path(appdata) / "com.ruut.stalker" / "config.json"


def autodetect_install() -> Path:
    cfg_path = launcher_config_path()
    if not cfg_path.is_file():
        sys.exit(f"{cfg_path} not found — pass --install explicitly.")
    cfg = json.loads(cfg_path.read_text(encoding="utf-8"))

    candidates: list[Path] = []
    for version in (cfg.get("installed_versions") or {}).values():
        path = version.get("installed_path") or ""
        if path and (Path(path) / "gamedata" / "configs" / CONFIG_LTX).is_file():
            candidates.append(Path(path))

    if not candidates:
        sys.exit(
            "No install with a faction_editor_config.ltx was found.\n"
            "The editor has to have been saved at least once — otherwise there is\n"
            "nothing to patch and the launcher correctly does nothing."
        )
    # Newest config wins: that is the install being played with.
    candidates.sort(key=lambda p: (p / "gamedata" / "configs" / CONFIG_LTX).stat().st_mtime, reverse=True)
    return candidates[0]


def patches_dir(install: Path) -> Path:
    return install / "appdata" / "patches"


def configs_dir(install: Path) -> Path:
    return install / "gamedata" / "configs"


def find_installed_patches(install: Path) -> list[tuple[str, Path]]:
    d = patches_dir(install)
    if not d.is_dir():
        return []
    out = []
    for f in d.glob("*.json"):
        try:
            data = json.loads(f.read_text(encoding="utf-8"))
        except Exception:
            continue
        out.append((data.get("installed_at") or "", data.get("name") or f.stem, f))
    # Same order the launcher uses (installed_at, then name) — sorting by file
    # name would put 0.5.10 before 0.5.9.
    out.sort()
    return [(name, f) for _, name, f in out]


# --------------------------------------------------------------------------
# Minimal ltx reading (same shape the launcher parses)
# --------------------------------------------------------------------------

def read_config(path: Path) -> str:
    raw = path.read_bytes()
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        # Configs written before the editor moved to UTF-8.
        text = raw.decode("cp1251")
    return text.lstrip("﻿")


def parse_pairs(text: str) -> dict[tuple[str, str], str]:
    """{(section, key): value} for patchable props, visuals sections skipped."""
    out: dict[tuple[str, str], str] = {}
    section = None
    skip = False
    for line in text.splitlines():
        header = re.match(r"^\s*\[([^\]]*)\]", line)
        if header:
            section = header.group(1).strip().lower()
            skip = VISUALS_MARKER in section
            continue
        if skip or section is None:
            continue
        payload = line.split(";", 1)[0]
        if "=" not in payload:
            continue
        key, value = payload.split("=", 1)
        key = key.strip().lower()
        if key in PATCHABLE_FIELDS:
            out[(section, key)] = value.strip()
    return out


def normalize(value: str) -> str:
    v = value.strip()
    if re.fullmatch(r"-?\d*\.?\d+", v) and "." in v:
        v = v.rstrip("0").rstrip(".")
        return v or "0"
    return v.lower()


def render_fragment(edits: list[tuple[str, str, str]]) -> bytes:
    """Engine ltx shape: 8-space indent, key padded to 32 columns, CRLF."""
    out: list[str] = []
    current = None
    for section, key, value in edits:
        if section != current:
            if current is not None:
                out.append("")
            out.append(f"[{section}]")
            current = section
        out.append(f"        {key:<32} = {value}")
    return ("\r\n".join(out) + "\r\n").encode("utf-8")


# --------------------------------------------------------------------------
# Building a fragment
# --------------------------------------------------------------------------

def bump(key: str, value: str) -> str | None:
    """A visibly different but still legal value for this prop."""
    v = value.strip()
    if key in ("fire_wound_preset", "explosion_preset"):
        return "strong" if v != "strong" else "normal"
    if key == "mutant_alliance":
        return "false" if v.lower() == "true" else "true"
    if key == "descr_diff":
        return str(int(v) % 5 + 1) if v.isdigit() else "3"
    if key.startswith("spot_color_"):
        return str((int(v) + 40) % 256) if v.isdigit() else "128"
    if re.fullmatch(r"-?\d+", v):
        return str(int(v) + max(1, abs(int(v)) // 10))
    if re.fullmatch(r"-?\d*\.\d+", v):
        # Immunities live in 0..1 and are stored inverted (1 - UI%/100).
        new = round(min(0.99, max(0.01, float(v) - 0.07)), 2)
        return f"{new:g}"
    return None


def build_small_fragment(config_text: str, count: int, only: list[str] | None = None) -> list[tuple[str, str, str]]:
    pairs = parse_pairs(config_text)
    if not pairs:
        sys.exit("The player config carries no patchable props — nothing to build a fragment from.")

    if only:
        unknown = [f for f in only if f not in PATCHABLE_FIELDS]
        if unknown:
            sys.exit(f"Not patchable, the launcher would reject these: {', '.join(unknown)}")
        pairs = {k: v for k, v in pairs.items() if k[1] in only}
        if not pairs:
            sys.exit(f"The player config carries none of: {', '.join(only)}")

    # Spread the picks over different sections and different props so the
    # dialog shows a realistic list rather than one prop repeated.
    edits: list[tuple[str, str, str]] = []
    used_keys: set[str] = set()
    used_sections: set[str] = set()
    for (section, key), value in sorted(pairs.items()):
        # One prop per section and each prop only once: that way the fragment
        # spans a base section AND rank sections, which is what a real balance
        # patch looks like, instead of six props of the same faction.
        if key in used_keys or section in used_sections:
            continue
        new_value = bump(key, value)
        if new_value is None or normalize(new_value) == normalize(value):
            continue
        edits.append((section, key, new_value))
        used_keys.add(key)
        used_sections.add(section)
        if len(edits) >= count:
            break

    return finish_edits(edits)


def finish_edits(edits: list[tuple[str, str, str]]) -> list[tuple[str, str, str]]:
    """Drop what the launcher's parse_fragment would reject, so the test bed
    never stages a fragment the launcher then refuses as FE_ERR_PATCH_INVALID."""
    kept, dropped = [], []
    for section, key, value in edits:
        if SECTION_RE.match(section) and VISUALS_MARKER not in section:
            kept.append((section, key, value))
        else:
            dropped.append(section)
    if dropped:
        print(f"note: skipped {len(dropped)} value(s) in sections the launcher would reject: {sorted(set(dropped))[:5]}")
    kept.sort()
    return kept


def build_full_diff(gamedata_repo: Path, old_rev: str, new_rev: str) -> list[tuple[str, str, str]]:
    def show(rev: str) -> str:
        res = subprocess.run(
            ["git", "show", f"{rev}:configs/{DEFAULT_CONFIG_LTX}"],
            cwd=gamedata_repo, capture_output=True,
        )
        if res.returncode != 0:
            sys.exit(f"git show {rev} failed: {res.stderr.decode('utf-8', 'replace')}")
        try:
            return res.stdout.decode("utf-8")
        except UnicodeDecodeError:
            return res.stdout.decode("cp1251")

    old = parse_pairs(show(old_rev))
    new = parse_pairs(show(new_rev))

    edits = [
        (section, key, value)
        for (section, key), value in new.items()
        if normalize(old.get((section, key), "")) != normalize(value)
    ]
    return finish_edits(edits)


# --------------------------------------------------------------------------
# Commands
# --------------------------------------------------------------------------

def cmd_show(args) -> None:
    install = Path(args.install) if args.install else autodetect_install()
    print(f"Install:  {install}")
    print(f"Config:   {configs_dir(install) / CONFIG_LTX}")
    print(f"Patches:  {patches_dir(install)}")
    print()

    patches = find_installed_patches(install)
    if not patches:
        print("No installed patches — install one through the launcher first,")
        print("or the test bed has no marker to attach a fragment to.")
        return

    for name, marker in patches:
        data = json.loads(marker.read_text(encoding="utf-8"))
        fields = data.get("fe_fields") or []
        fragment = patches_dir(install) / f"{name}{FRAGMENT_SUFFIX}"
        print(f"  {name}")
        print(f"    installed_at : {data.get('installed_at')}")
        print(f"    fe_fields    : {len(fields)} {fields if fields else ''}")
        print(f"    fe_applied_at: {data.get('fe_applied_at')}")
        print(f"    fragment     : {'present' if fragment.is_file() else 'absent'}")


def cmd_stage(args) -> None:
    install = Path(args.install) if args.install else autodetect_install()
    config_path = configs_dir(install) / CONFIG_LTX
    if not config_path.is_file():
        sys.exit(f"{config_path} not found — open the faction editor in game and save once.")

    patches = find_installed_patches(install)
    if not patches:
        sys.exit("No installed patch to attach the fragment to. Install one through the launcher first.")

    if args.patch:
        match = [p for p in patches if p[0] == args.patch]
        if not match:
            sys.exit(f"Patch '{args.patch}' is not installed. Known: {', '.join(n for n, _ in patches)}")
        name, marker = match[0]
    else:
        name, marker = patches[-1]

    config_text = read_config(config_path)

    if args.full_diff:
        repo = Path(args.gamedata_repo)
        if not (repo / ".git").exists():
            sys.exit(f"{repo} is not a git repository — pass --gamedata-repo.")
        edits = build_full_diff(repo, args.old_rev, args.new_rev)
        source = f"git diff {args.old_rev}..{args.new_rev} of {DEFAULT_CONFIG_LTX}"
    else:
        only = [f.strip() for f in args.fields.split(",") if f.strip()] if args.fields else None
        edits = build_small_fragment(config_text, args.count, only)
        source = f"{len(edits)} props taken from the player's own config and bumped"
        if only:
            source += f" (limited to {', '.join(only)})"

    if not edits:
        sys.exit("The diff produced no patchable changes — nothing to stage.")

    if args.with_unknown_section:
        # Exercises FE_WARN_PATCH_SKIPPED_SECTIONS: a faction the mod added
        # after the player's config was written.
        edits.append(("faction_42", "power", "7"))
        edits.sort()

    fragment_path = patches_dir(install) / f"{name}{FRAGMENT_SUFFIX}"
    fragment_path.parent.mkdir(parents=True, exist_ok=True)
    fragment_path.write_bytes(render_fragment(edits))

    fields = sorted({key for _, key, _ in edits})
    data = json.loads(marker.read_text(encoding="utf-8"))
    data["fe_fields"] = fields
    data["fe_fragment"] = fragment_path.name
    data["fe_applied_at"] = None
    marker.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")

    current = parse_pairs(config_text)
    print(f"Staged onto patch '{name}'")
    print(f"  source   : {source}")
    print(f"  fragment : {fragment_path}  ({fragment_path.stat().st_size} bytes)")
    print(f"  marker   : {marker}")
    print(f"  props    : {len(fields)} -> {fields}")
    print(f"  values   : {len(edits)}")
    print()
    print("Expected effect on the player's config (first 20):")
    for section, key, value in edits[:20]:
        before = current.get((section, key))
        before = "<absent>" if before is None else before
        print(f"  [{section}] {key}: {before} -> {value}")
    print()
    print("Now, in the launcher: Версии -> expand this version -> the installed patch row")
    print("has an 'Применить настройки' button. Or reinstall the patch to see the dialog")
    print("that pops up right after an install.")
    print()
    print("Undo with:  python scripts/fe_patch_testbed.py clean")


def cmd_clean(args) -> None:
    install = Path(args.install) if args.install else autodetect_install()
    removed = 0

    for name, marker in find_installed_patches(install):
        if args.patch and name != args.patch:
            continue
        fragment = patches_dir(install) / f"{name}{FRAGMENT_SUFFIX}"
        if fragment.is_file():
            fragment.unlink()
            print(f"removed {fragment}")
            removed += 1
        data = json.loads(marker.read_text(encoding="utf-8"))
        if any(k in data for k in ("fe_fields", "fe_fragment", "fe_applied_at")):
            for k in ("fe_fields", "fe_fragment", "fe_applied_at"):
                data.pop(k, None)
            marker.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
            print(f"cleaned {marker}")
            removed += 1

    # Fragments whose marker is gone (or a stray staging file) have no patch
    # to belong to; sweep them too so the folder returns to a clean state.
    d = patches_dir(install)
    if d.is_dir() and not args.patch:
        for f in d.glob(f"*{FRAGMENT_SUFFIX}"):
            f.unlink()
            print(f"removed orphan {f}")
            removed += 1

    if removed == 0:
        print("Nothing to clean.")
    else:
        print()
        print("Note: this does NOT undo settings already applied to the config.")
        print("For that, restore the .gwfe backup the apply wrote:")
        print("  Редактор фракций -> Импорт и применить -> pick the newest file in")
        print("  %APPDATA%\\com.ruut.stalker\\faction-profiles\\_backup\\")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--install", help="Game install dir (default: autodetected from the launcher config)")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_show = sub.add_parser("show", help="What the launcher currently sees")
    p_show.set_defaults(func=cmd_show)

    p_stage = sub.add_parser("stage", help="Attach a settings fragment to an installed patch")
    p_stage.add_argument("--patch", help="Patch tag (default: the newest installed one)")
    p_stage.add_argument("--count", type=int, default=6, help="How many props to change (default: 6)")
    p_stage.add_argument("--fields", help="Comma-separated prop names to limit the fragment to")
    p_stage.add_argument("--full-diff", action="store_true", help="Use a real git diff of the reference config instead")
    p_stage.add_argument("--old-rev", default="0.5.2-Beta", help="Base revision for --full-diff")
    p_stage.add_argument("--new-rev", default="HEAD", help="New revision for --full-diff")
    p_stage.add_argument(
        "--gamedata-repo",
        default=str(Path(__file__).resolve().parents[2] / "GlobalWar" / "gamedata"),
        help="Path to the gamedata git repo (for --full-diff)",
    )
    p_stage.add_argument(
        "--with-unknown-section",
        action="store_true",
        help="Also add a section the player's config lacks, to exercise the 'skipped sections' warning",
    )
    p_stage.set_defaults(func=cmd_stage)

    p_clean = sub.add_parser("clean", help="Remove staged fragments and marker fields")
    p_clean.add_argument("--patch", help="Only this patch tag")
    p_clean.set_defaults(func=cmd_clean)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
