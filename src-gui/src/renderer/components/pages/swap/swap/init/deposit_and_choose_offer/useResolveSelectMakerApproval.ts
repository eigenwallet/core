import { useCallback } from "react";
import { store, type RootState } from "renderer/store/storeRenderer";
import { resolveApproval } from "renderer/rpc";
import { isPendingSelectMakerApprovalEvent } from "models/tauriModelExt";

const WAIT_FOR_APPROVAL_MS = 5_000;

function findPendingApprovalForPeer(
  state: RootState,
  peerId: string,
): string | null {
  const requests = Object.values(state.rpc.state.approvalRequests);
  for (const req of requests) {
    if (req.request_status.state !== "Pending") continue;
    if (!isPendingSelectMakerApprovalEvent(req)) continue;
    if (req.request.content.maker.peer_id === peerId) return req.request_id;
  }
  return null;
}

/**
 * The snapshot list shown to the user can hold a request id that has already
 * expired by the time they click Select. In that case we wait briefly for the
 * backend to emit a fresh pending approval for the same peer and resolve that
 * one instead, so a stale snapshot doesn't surface as a confusing error.
 */
export function useResolveSelectMakerApproval() {
  return useCallback(
    async (peerId: string, snapshotRequestId: string | undefined) => {
      const tryResolve = (id: string) =>
        resolveApproval(id, true as unknown as object);

      if (snapshotRequestId) {
        const stillPending =
          store.getState().rpc.state.approvalRequests[snapshotRequestId]
            ?.request_status.state === "Pending";
        if (stillPending) return tryResolve(snapshotRequestId);
      }

      const immediate = findPendingApprovalForPeer(store.getState(), peerId);
      if (immediate) return tryResolve(immediate);

      const freshId = await new Promise<string | null>((resolve) => {
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
      });

      if (!freshId) {
        throw new Error(
          "This offer is no longer available. Please pick another maker.",
        );
      }
      return tryResolve(freshId);
    },
    [],
  );
}
