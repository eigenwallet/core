import { useCallback } from "react";
import { store, type RootState } from "renderer/store/storeRenderer";
import { resolveApproval } from "renderer/rpc";
import {
  isPendingSelectMakerApprovalEvent,
  PendingSelectMakerApprovalRequest,
} from "models/tauriModelExt";

const WAIT_FOR_APPROVAL_MS = 5_000;

// How far the price of the approval we resolve may be above the price the
// card displayed, in parts per thousand
const MAX_PRICE_INCREASE_SINCE_DISPLAY_PER_MILLE = 2;

function findPendingApprovalForPeer(
  state: RootState,
  peerId: string,
): PendingSelectMakerApprovalRequest | null {
  const requests = Object.values(state.rpc.state.approvalRequests);
  for (const req of requests) {
    if (!isPendingSelectMakerApprovalEvent(req)) continue;
    if (req.request.content.maker.peer_id === peerId) return req;
  }
  return null;
}

function findPendingApprovalById(
  state: RootState,
  requestId: string,
): PendingSelectMakerApprovalRequest | null {
  const req = state.rpc.state.approvalRequests[requestId];
  if (req == null || !isPendingSelectMakerApprovalEvent(req)) return null;
  return req;
}

/**
 * The snapshot list shown to the user can hold a request id that has already
 * expired by the time they click Select. In that case we wait briefly for the
 * backend to emit a fresh pending approval for the same peer and resolve that
 * one instead, so a stale snapshot doesn't surface as a confusing error.
 *
 * An approval keeps its quote for as long as it exists. We only resolve an
 * approval whose price is at most a small tolerance above the price the card
 * displayed, so the user never accepts a price they did not see.
 */
export function useResolveSelectMakerApproval() {
  return useCallback(
    async (
      peerId: string,
      snapshotRequestId: string | undefined,
      displayedPrice: number,
    ) => {
      const resolveAtDisplayedPrice = (
        approval: PendingSelectMakerApprovalRequest,
      ) => {
        const approvalPrice = approval.request.content.maker.quote.price;
        if (
          approvalPrice * 1000 >
          displayedPrice * (1000 + MAX_PRICE_INCREASE_SINCE_DISPLAY_PER_MILLE)
        ) {
          throw new Error("The offer changed. Review the current offer.");
        }
        return resolveApproval(approval.request_id, true as unknown as object);
      };

      const snapshot =
        snapshotRequestId == null
          ? null
          : findPendingApprovalById(store.getState(), snapshotRequestId);
      if (snapshot) return resolveAtDisplayedPrice(snapshot);

      const immediate = findPendingApprovalForPeer(store.getState(), peerId);
      if (immediate) return resolveAtDisplayedPrice(immediate);

      const fresh = await new Promise<PendingSelectMakerApprovalRequest | null>(
        (resolve) => {
          const timeout = window.setTimeout(() => {
            unsubscribe();
            resolve(null);
          }, WAIT_FOR_APPROVAL_MS);

          const unsubscribe = store.subscribe(() => {
            const found = findPendingApprovalForPeer(store.getState(), peerId);
            if (found) {
              window.clearTimeout(timeout);
              unsubscribe();
              resolve(found);
            }
          });
        },
      );

      if (!fresh) {
        throw new Error(
          "This offer is no longer available. Please pick another maker.",
        );
      }
      return resolveAtDisplayedPrice(fresh);
    },
    [],
  );
}
