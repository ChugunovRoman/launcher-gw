export function getInGb(value: number): string {
  return (value / 1024 / 1024 / 1024).toFixed(2);
}
export function getInMb(value: number): string {
  return (value / 1024 / 1024).toFixed(2);
}
export function parseBytes(value?: number): [number, string] {
  if (!value) {
    return [0, "bSfx"];
  }

  if (value >= 1_000_000_000) {
    return [Number((value / 1024 / 1024 / 1024).toFixed(2)), "gbSfx"];
  } else if (value >= 1_000_000) {
    return [Number((value / 1024 / 1024).toFixed(2)), "mbSfx"];
  } else if (value >= 1_000) {
    return [Number((value / 1024).toFixed(2)), "kbSfx"];
  } else {
    return [Number(value.toFixed(2)), "bSfx"];
  }
}
export function formatSpeedBytesPerSec(bitsPerSec: number): [number, string] {
  if (bitsPerSec >= 1_000_000) {
    return [Number((bitsPerSec / 1024 / 1024).toFixed(2)), "МБ/с"];
  } else if (bitsPerSec >= 1_000) {
    return [Number((bitsPerSec / 1024).toFixed(2)), "КБ/с "];
  } else {
    return [Number(bitsPerSec.toFixed(2)), "Б/с "];
  }
}
