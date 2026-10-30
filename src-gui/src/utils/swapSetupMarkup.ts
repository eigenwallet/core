import { getMarkup } from "./conversionUtils";

/// The offered markup above the market rate, in percent, that causes a warning.
export const MAX_OFFERED_MARKUP_PERCENT = 5;

/// The offered price above the quoted price, in percent, that causes a warning.
export const MAX_PRICE_INCREASE_OVER_QUOTE_PERCENT = 5;

/// The increase of the markup from the quote to the offer, as a fraction of the
/// quoted markup, that causes a warning.
export const MAX_MARKUP_INCREASE_FRACTION = 0.25;

export type SwapSetupRateWarning =
  | { type: "MarkupHigh" | "MarkupHighAndRaised"; offeredMarkupPercent: number }
  | { type: "MarkupRaised" | "RateRaised" | "NoQuote" };

/// Returns the warning for an offer, or null if there is nothing to warn about.
///
/// The markup is high if the offered markup is more than 5%. It is raised if
/// the offered markup is more than 25% above the quoted markup, or the offered
/// rate is more than 5% above the quoted rate.
///
/// | Offered markup | Not raised | Raised                |
/// |----------------|------------|-----------------------|
/// | Up to 5%       | null       | `MarkupRaised`        |
/// | Above 5%       | `MarkupHigh` | `MarkupHighAndRaised` |
///
/// Without a market rate there is no markup, so the only warning is `RateRaised`
/// if the offered rate is more than 5% above the quoted rate. Without a quote
/// there is nothing to compare the offer with, so the warning is `NoQuote`.
///
/// All rates are in BTC per XMR. `quotedRate` is null if the GUI did not
/// receive a quote for this swap. `marketRate` is null if the user turned off
/// price fetching.
export function getSwapSetupRateWarning({
  quotedRate,
  offeredRate,
  marketRate,
}: {
  quotedRate: number | null;
  offeredRate: number;
  marketRate: number | null;
}): SwapSetupRateWarning | null {
  if (quotedRate == null) {
    return { type: "NoQuote" };
  }

  const rateRaised =
    getMarkup(offeredRate, quotedRate) > MAX_PRICE_INCREASE_OVER_QUOTE_PERCENT;

  if (marketRate == null) {
    return rateRaised ? { type: "RateRaised" } : null;
  }

  const quotedMarkup = getMarkup(quotedRate, marketRate);
  const offeredMarkup = getMarkup(offeredRate, marketRate);

  const markupHigh = offeredMarkup > MAX_OFFERED_MARKUP_PERCENT;
  const markupRaised =
    rateRaised ||
    offeredMarkup - quotedMarkup >
      MAX_MARKUP_INCREASE_FRACTION * Math.abs(quotedMarkup);

  if (markupHigh) {
    return {
      type: markupRaised ? "MarkupHighAndRaised" : "MarkupHigh",
      offeredMarkupPercent: offeredMarkup,
    };
  }

  return markupRaised ? { type: "MarkupRaised" } : null;
}
