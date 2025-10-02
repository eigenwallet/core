const FNV_OFFSET_BASIS = 0x811c9dc5;
const FNV_PRIME = 0x01000193;

export function fnv1a(value: string): string {
  let hash = FNV_OFFSET_BASIS;

  for (let i = 0; i < value.length; i += 1) {
    hash ^= value.charCodeAt(i);
    hash = (hash * FNV_PRIME) >>> 0;
  }

  return hash.toString(16).padStart(8, "0");
}