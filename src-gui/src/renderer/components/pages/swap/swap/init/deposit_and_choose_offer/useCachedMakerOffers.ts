import { useEffect, useMemo, useRef, useState } from "react";
import { QuoteWithAddress, RefundPolicyWire } from "models/tauriModel";
import { usePendingSelectMakerApproval } from "store/hooks";
import {
  OfferSortMode,
  SortedMakerEntry,
  sortApprovalsAndKnownQuotes,
} from "utils/sortUtils";

const REFRESH_INTERVAL_MS = 5_000;

function refundPolicyEqual(a: RefundPolicyWire, b: RefundPolicyWire): boolean {
  if (a.type !== b.type) return false;
  if (a.type === "FullRefund") return true;
  return (
    a.content.anti_spam_deposit_ratio ===
    (b as Extract<RefundPolicyWire, { type: "PartialRefund" }>).content
      .anti_spam_deposit_ratio
  );
}

function offersEqual(a: SortedMakerEntry[], b: SortedMakerEntry[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    const x = a[i];
    const y = b[i];
    if (x.isDuplicate !== y.isDuplicate) return false;
    if (x.approval?.request_id !== y.approval?.request_id) return false;
    const xq = x.quote_with_address;
    const yq = y.quote_with_address;
    if (xq.peer_id !== yq.peer_id) return false;
    if (xq.multiaddr !== yq.multiaddr) return false;
    if (xq.version !== yq.version) return false;
    if (xq.quote.price !== yq.quote.price) return false;
    if (xq.quote.min_quantity !== yq.quote.min_quantity) return false;
    if (xq.quote.max_quantity !== yq.quote.max_quantity) return false;
    if (!refundPolicyEqual(xq.quote.refund_policy, yq.quote.refund_policy))
      return false;
  }
  return true;
}

// The sorted list re-shuffles whenever the backend streams an approval or
// quote update. We snapshot it and only refresh on a fixed cadence so cards
// don't visibly flicker. The snapshot is also refreshed immediately on
// sort-mode change and whenever a new peer appears, so newly-discovered
// makers don't get stuck behind the cadence.
export function useCachedMakerOffers(
  known_quotes: QuoteWithAddress[],
  sortMode: OfferSortMode,
  offersPerPage: number,
): SortedMakerEntry[] {
  const pendingApprovals = usePendingSelectMakerApproval();

  const liveOffers = useMemo(
    () =>
      sortApprovalsAndKnownQuotes(
        pendingApprovals,
        known_quotes,
        sortMode,
        offersPerPage,
      ),
    [pendingApprovals, known_quotes, sortMode, offersPerPage],
  );

  const liveOffersRef = useRef<SortedMakerEntry[]>(liveOffers);
  liveOffersRef.current = liveOffers;

  const [snapshot, setSnapshot] = useState<SortedMakerEntry[]>(liveOffers);

  useEffect(() => {
    setSnapshot(liveOffersRef.current);
  }, [sortMode]);

  useEffect(() => {
    const snapshotPeers = new Set(
      snapshot.map((o) => o.quote_with_address.peer_id),
    );
    const hasNewPeer = liveOffers.some(
      (o) => !snapshotPeers.has(o.quote_with_address.peer_id),
    );
    if (hasNewPeer) setSnapshot(liveOffers);
  }, [snapshot, liveOffers]);

  useEffect(() => {
    const id = window.setInterval(() => {
      setSnapshot((prev) =>
        offersEqual(prev, liveOffersRef.current) ? prev : liveOffersRef.current,
      );
    }, REFRESH_INTERVAL_MS);
    return () => window.clearInterval(id);
  }, []);

  return snapshot;
}
