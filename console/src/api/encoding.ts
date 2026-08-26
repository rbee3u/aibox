export function decodeBase64(value: string): Uint8Array {
  const binary = window.atob(value);
  return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}

/** Returns `null` instead of throwing when `value` is not valid base64. */
export function tryDecodeBase64(value: string): Uint8Array | null {
  try {
    return decodeBase64(value);
  } catch {
    return null;
  }
}

export function encodeBase64(value: Uint8Array): string {
  let binary = "";
  for (let index = 0; index < value.length; index += 0x8000) {
    binary += String.fromCharCode(...value.subarray(index, index + 0x8000));
  }
  return window.btoa(binary);
}

/**
 * Binary-prefix byte sizes (KiB/MiB). The Console also renders decimal-prefix
 * sizes through `formatByteSize` in `shared/lib/format`; the two spellings are
 * deliberately kept apart so existing surfaces keep their exact wording.
 */
export function formatBinaryByteSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}
