import { describe, expect, it } from "vitest";
import { getExchangeRate, satsToBtc } from "./conversionUtils";
import { shouldWarnAboutSwapSetupRate } from "./swapSetupMarkup";

const MARKET = 0.006;

describe("shouldWarnAboutSwapSetupRate", () => {
  it("accepts an offer at the quoted rate", () => {
    expect(
      shouldWarnAboutSwapSetupRate({
        quotedRate: 0.00612,
        offeredRate: 0.00612,
        marketRate: MARKET,
      }),
    ).toBe(false);
  });

  it("accepts an ASB quote that is truncated to whole piconeros", () => {
    expect(
      shouldWarnAboutSwapSetupRate({
        quotedRate: satsToBtc(630_000),
        offeredRate: getExchangeRate(50_000_000, 79_365_079_365_079 + 1),
        marketRate: MARKET,
      }),
    ).toBe(false);
  });

  it("warns if the offered markup is more than 25% above the quoted markup", () => {
    // Quoted markup 2%, offered markup 2.6%.
    expect(
      shouldWarnAboutSwapSetupRate({
        quotedRate: 0.00612,
        offeredRate: 0.006156,
        marketRate: MARKET,
      }),
    ).toBe(true);
  });

  it("accepts an offered markup up to 25% above the quoted markup", () => {
    // Quoted markup 2%, offered markup 2.4%.
    expect(
      shouldWarnAboutSwapSetupRate({
        quotedRate: 0.00612,
        offeredRate: 0.006144,
        marketRate: MARKET,
      }),
    ).toBe(false);
  });

  it("warns if the offered markup is above 5%, also if the quote had the same markup", () => {
    // Quoted and offered markup 6%.
    expect(
      shouldWarnAboutSwapSetupRate({
        quotedRate: 0.00636,
        offeredRate: 0.00636,
        marketRate: MARKET,
      }),
    ).toBe(true);
  });

  it("accepts an unchanged rate with a negative quoted markup", () => {
    // Quoted and offered markup -1%.
    expect(
      shouldWarnAboutSwapSetupRate({
        quotedRate: 0.00594,
        offeredRate: 0.00594,
        marketRate: MARKET,
      }),
    ).toBe(false);
  });

  it("warns if the offered rate is more than 5% above the quote without a market rate", () => {
    expect(
      shouldWarnAboutSwapSetupRate({
        quotedRate: 0.006,
        offeredRate: 0.00631,
        marketRate: null,
      }),
    ).toBe(true);
  });

  it("accepts an offered rate up to 5% above the quote without a market rate", () => {
    expect(
      shouldWarnAboutSwapSetupRate({
        quotedRate: 0.006,
        offeredRate: 0.0063,
        marketRate: null,
      }),
    ).toBe(false);
  });

  it("warns if there is no quote", () => {
    expect(
      shouldWarnAboutSwapSetupRate({
        quotedRate: null,
        offeredRate: 0.006,
        marketRate: MARKET,
      }),
    ).toBe(true);
  });
});
