import exolixAvatar from "assets/exolix.svg";

export interface PriorityMaker {
  avatar: string;
}

export const PRIORITY_MAKERS: Record<string, PriorityMaker> = {
  "12D3KooWBk6GbgkZaeTAUByD1tJX6SdFHtzrVj3jTmurPMRvtGoY": {
    avatar: exolixAvatar
  },
};

export function getPriorityMaker(peerId: string): PriorityMaker | undefined {
  return PRIORITY_MAKERS[peerId];
}

export function isPriorityMaker(peerId: string): boolean {
  return peerId in PRIORITY_MAKERS;
}
