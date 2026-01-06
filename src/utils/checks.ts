import { get } from "svelte/store";
import {
  localVersions,
} from "../store/main";

export function hasLocalVersion(version: Version) {
  for (const [name, local] of get(localVersions)) {
    if (name === version.name) return true;
    if (local.path === version.name) return true;
    if (local.path === version.path) return true;
  }

  return false;
}
