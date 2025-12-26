import { open } from "@tauri-apps/plugin-dialog";

export async function choosePath(cb: (path: string) => void) {
  const selected = await open({
    directory: true,
    multiple: false,
  });
  if (selected) {
    cb(selected);
  }
}
