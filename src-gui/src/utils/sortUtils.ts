import {
  PendingSelectMakerApprovalRequest,
  SortableQuoteWithAddress,
} from "models/tauriModelExt";
import { QuoteWithAddress } from "models/tauriModel";
import { isMakerVersionOutdated } from "./multiAddrUtils";
import _ from "lodash";

export function sortApprovalsAndKnownQuotes(
  pendingSelectMakerApprovals: PendingSelectMakerApprovalRequest[],
  known_quotes: QuoteWithAddress[],
) {
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

  return (
    _(sortableQuotes)
      .orderBy(
        [
          // Prefer makers that have a 'version' attribute
          // If we don't have a version, we cannot clarify if it's outdated or not
          (m) => (m.quote_with_address.version ? 0 : 1),
          // Prefer makers with a minimum quantity > 0
          (m) => ((m.quote_with_address.quote.min_quantity ?? 0) > 0 ? 0 : 1),
          // Prefer makers that are not outdated
          (m) => (isMakerVersionOutdated(m.quote_with_address.version) ? 1 : 0),
          // Prefer approvals over actual quotes
          (m) => (m.approval ? 0 : 1),
          // Prefer makers with a lower price
          (m) => m.quote_with_address.quote.price,
        ],
        ["asc", "asc", "asc", "asc", "asc"],
      )
      // Remove duplicate makers
      .uniqBy((m) => m.quote_with_address.peer_id)
      .value()
  );
}
