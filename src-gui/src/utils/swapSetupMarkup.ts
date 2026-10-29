import { getMarkup } from "./conversionUtils";

/// The offered markup above the market rate, in percent, that causes a warning.
export const MAX_OFFERED_MARKUP_PERCENT = 5;

/// The offered price above the quoted price, in percent, that causes a warning.
export const MAX_PRICE_INCREASE_OVER_QUOTE_PERCENT = 5;

/// The increase of the markup from the quote to the offer, as a fraction of the
/// quoted markup, that causes a warning.
export const MAX_MARKUP_INCREASE_FRACTION = 0.25;

/// Returns true if one of these conditions applies:
/// - The offered rate is more than 5% above the quoted rate.
/// - The offered markup is more than 5%.
/// - The offered markup is more than 25% above the quoted markup.
/// - The GUI did not receive a quote.
///
/// All rates are in BTC per XMR. `quotedRate` is null if the GUI did not
/// receive a quote for this swap. `marketRate` is null if the user turned off
/// price fetching.
export function shouldWarnAboutSwapSetupRate({
  quotedRate,
  offeredRate,
  marketRate,
}: {
  quotedRate: number | null;
  offeredRate: number;
  marketRate: number | null;
}): boolean {
  if (quotedRate == null) {
    return true;
  }

  if (
    getMarkup(offeredRate, quotedRate) > MAX_PRICE_INCREASE_OVER_QUOTE_PERCENT
  ) {
    return true;
  }

  if (marketRate == null) {
    return false;
  }

  const quotedMarkup = getMarkup(quotedRate, marketRate);
  const offeredMarkup = getMarkup(offeredRate, marketRate);

  if (offeredMarkup > MAX_OFFERED_MARKUP_PERCENT) {
    return true;
  }

  return (
    offeredMarkup - quotedMarkup >
    MAX_MARKUP_INCREASE_FRACTION * Math.abs(quotedMarkup)
  );
}
