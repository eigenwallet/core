export const PRIORITY_MAKERS = new Set([
  "12D3KooWQQeUXdMkwJo8zESkKo7xek7NtVgqh1Q4RPnJCTmESX1Z",
]);

export function isPriorityMaker(peerId: string): boolean {
  return PRIORITY_MAKERS.has(peerId);
}
