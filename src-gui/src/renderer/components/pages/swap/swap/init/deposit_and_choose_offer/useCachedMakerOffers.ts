import { useEffect, useMemo, useRef, useState } from "react";
import { QuoteWithAddress } from "models/tauriModel";
import { useAppSelector, usePendingSelectMakerApproval } from "store/hooks";
import _ from "lodash";
import {
  OfferSortMode,
  SortedMakerEntry,
  sortApprovalsAndKnownQuotes,
} from "utils/sortUtils";

const REFRESH_INTERVAL_MS = 5_000;

// The sorted list re-shuffles whenever the backend streams an approval or
// quote update. We snapshot it and only refresh on a fixed cadence so cards
// don't visibly flicker. The snapshot is also refreshed immediately on
// sort-mode change, on Bitcoin balance change, and whenever a new peer
// appears, so newly-discovered makers and freshly-deposited funds don't get
// stuck behind the cadence.
export function useCachedMakerOffers(
  known_quotes: QuoteWithAddress[],
  sortMode: OfferSortMode,
  offersPerPage: number,
): SortedMakerEntry[] {
  const pendingApprovals = usePendingSelectMakerApproval();
  const bitcoinBalance = useAppSelector((state) => state.bitcoinWallet.balance);

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
  }, [sortMode, bitcoinBalance]);

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
        _.isEqual(prev, liveOffersRef.current) ? prev : liveOffersRef.current,
      );
    }, REFRESH_INTERVAL_MS);
    return () => window.clearInterval(id);
  }, []);

  return snapshot;
}
