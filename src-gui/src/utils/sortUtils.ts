import {
  PendingSelectMakerApprovalRequest,
  SortableQuoteWithAddress,
} from "models/tauriModelExt";
import { QuoteWithAddress } from "models/tauriModel";
import { isMakerVersionOld, isMakerVersionTooOld } from "./multiAddrUtils";
import { isPriorityMaker } from "./priorityMakers";
import _ from "lodash";

export type OfferSortMode = "large" | "small" | "cheapest";

export type SortedMakerEntry = SortableQuoteWithAddress & {
  /** True when this entry is a priority maker re-shown at its natural position. */
  isDuplicate: boolean;
};

function sortNaturally(
  quotes: SortableQuoteWithAddress[],
  sortMode: OfferSortMode,
): SortableQuoteWithAddress[] {
  const primaryIteratee = (m: SortableQuoteWithAddress) => {
    const q = m.quote_with_address.quote;
    switch (sortMode) {
      case "large":
        return -q.max_quantity;
      case "small":
        return q.min_quantity;
      case "cheapest":
        return q.price;
    }
  };

  return _(quotes)
    .orderBy(
      [
        // Prefer makers that have a 'version' attribute
        // If we don't have a version, we cannot clarify if it's outdated or not
        (m) => (m.quote_with_address.version ? 0 : 1),
        // Prefer makers with a max quantity > 0 (have liquidity)
        (m) => (m.quote_with_address.quote.max_quantity > 0 ? 0 : 1),
        // Prefer makers with a minimum quantity > 0
        (m) => (m.quote_with_address.quote.min_quantity > 0 ? 0 : 1),
        // Prefer makers that are not incompatible
        (m) =>
          isMakerVersionTooOld(m.quote_with_address.version)
            ? 2
            : isMakerVersionOld(m.quote_with_address.version)
              ? 1
              : 0,
        // Prefer approvals over actual quotes
        (m) => (m.approval ? 0 : 1),
        // User-selected sort criterion
        primaryIteratee,
      ],
      ["asc", "asc", "asc", "asc", "asc", "asc"],
    )
    .uniqBy((m) => m.quote_with_address.peer_id)
    .value();
}

export function sortApprovalsAndKnownQuotes(
  pendingSelectMakerApprovals: PendingSelectMakerApprovalRequest[],
  known_quotes: QuoteWithAddress[],
  sortMode: OfferSortMode = "large",
  offersPerPage: number = Infinity,
): SortedMakerEntry[] {
  const sortableQuotes: SortableQuoteWithAddress[] =
    pendingSelectMakerApprovals.map((approval) => {
      return {
        quote_with_address: approval.request.content.maker,
        approval:
          approval.request_status.state === "Pending"
            ? {
                request_id: approval.request_id,
                expiration_ts: approval.request_status.content.expiration_ts,
              }
            : null,
      };
    });

  sortableQuotes.push(
    ...known_quotes.map((quote) => ({
      quote_with_address: quote,
      approval: null,
    })),
  );

  const naturalSorted = sortNaturally(sortableQuotes, sortMode);

  // Priority makers (with liquidity), in their natural order.
  const priorityList = naturalSorted.filter(
    (m) =>
      isPriorityMaker(m.quote_with_address.peer_id) &&
      m.quote_with_address.quote.max_quantity > 0,
  );
  const priorityPeerIds = new Set(
    priorityList.map((m) => m.quote_with_address.peer_id),
  );

  const result: SortedMakerEntry[] = priorityList.map((m) => ({
    ...m,
    isDuplicate: false,
  }));

  // Append the natural list. If a priority maker would naturally land on
  // page 1, drop it here so it isn't shown twice on the same page. Otherwise
  // keep it at its natural position and flag the second occurrence as a
  // duplicate (e.g., for distinct React keys).
  naturalSorted.forEach((m, idx) => {
    const isPriority = priorityPeerIds.has(m.quote_with_address.peer_id);
    if (isPriority && idx < offersPerPage) return;
    result.push({ ...m, isDuplicate: isPriority });
  });

  return result;
}
