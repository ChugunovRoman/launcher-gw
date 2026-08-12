import { open } from "@tauri-apps/plugin-dialog";

export async function choosePath(cb: (path: string) => void): Promise<string | undefined> {
  const selected = await open({
    directory: true,
    multiple: false,
  });
  if (selected) {
    cb(selected);

    return selected;
  }

  return;
}

export async function chooseFilePath(cb: (path: string) => void): Promise<string | undefined> {
  const selected = await open({
    directory: false,
    multiple: false,
  });
  if (selected) {
    cb(selected);

    return selected;
  }

  return;
}
