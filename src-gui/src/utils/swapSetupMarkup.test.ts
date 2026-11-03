import { describe, expect, it } from "vitest";
import { getExchangeRate, satsToBtc } from "./conversionUtils";
import { getSwapSetupRateWarning } from "./swapSetupMarkup";

const MARKET = 0.006;

describe("getSwapSetupRateWarning", () => {
  it("accepts an offer at the quoted rate", () => {
    expect(
      getSwapSetupRateWarning({
        quotedRate: 0.00612,
        offeredRate: 0.00612,
        marketRate: MARKET,
      }),
    ).toBeNull();
  });

  it("accepts an ASB quote that is truncated to whole piconeros", () => {
    expect(
      getSwapSetupRateWarning({
        quotedRate: satsToBtc(630_000),
        offeredRate: getExchangeRate(50_000_000, 79_365_079_365_079 + 1),
        marketRate: MARKET,
      }),
    ).toBeNull();
  });

  it("warns if the offered markup is more than 25% above the quoted markup", () => {
    // Quoted markup 2%, offered markup 2.6%.
    expect(
      getSwapSetupRateWarning({
        quotedRate: 0.00612,
        offeredRate: 0.006156,
        marketRate: MARKET,
      })?.type,
    ).toBe("MarkupRaised");
  });

  it("accepts an offered markup up to 25% above the quoted markup", () => {
    // Quoted markup 2%, offered markup 2.4%.
    expect(
      getSwapSetupRateWarning({
        quotedRate: 0.00612,
        offeredRate: 0.006144,
        marketRate: MARKET,
      }),
    ).toBeNull();
  });

  it("warns if the offered markup is above 5%, also if the quote had the same markup", () => {
    // Quoted and offered markup 6%.
    const warning = getSwapSetupRateWarning({
      quotedRate: 0.00636,
      offeredRate: 0.00636,
      marketRate: MARKET,
    });

    expect(warning?.type).toBe("MarkupHigh");
    expect(warning).toHaveProperty("offeredMarkupPercent");
  });

  it("warns if the offered markup is above 5% and more than 25% above the quoted markup", () => {
    // Quoted markup 2%, offered markup 12%.
    expect(
      getSwapSetupRateWarning({
        quotedRate: 0.00612,
        offeredRate: 0.00672,
        marketRate: MARKET,
      })?.type,
    ).toBe("MarkupHighAndRaised");
  });

  it("accepts a small increase of a low quoted markup", () => {
    // Quoted markup 0.1%, offered markup 0.3%.
    expect(
      getSwapSetupRateWarning({
        quotedRate: 0.006006,
        offeredRate: 0.006018,
        marketRate: MARKET,
      }),
    ).toBeNull();
  });

  it("warns if a low quoted markup increases by more than 0.25 percentage points", () => {
    // Quoted markup 0.1%, offered markup 0.4%.
    expect(
      getSwapSetupRateWarning({
        quotedRate: 0.006006,
        offeredRate: 0.006024,
        marketRate: MARKET,
      })?.type,
    ).toBe("MarkupRaised");
  });

  it("accepts a small increase of a negative quoted markup", () => {
    // Quoted markup -1%, offered markup -0.8%.
    expect(
      getSwapSetupRateWarning({
        quotedRate: 0.00594,
        offeredRate: 0.005952,
        marketRate: MARKET,
      }),
    ).toBeNull();
  });

  it("accepts an unchanged rate with a negative quoted markup", () => {
    // Quoted and offered markup -1%.
    expect(
      getSwapSetupRateWarning({
        quotedRate: 0.00594,
        offeredRate: 0.00594,
        marketRate: MARKET,
      }),
    ).toBeNull();
  });

  it("warns if the offered rate is more than 5% above the quote without a market rate", () => {
    expect(
      getSwapSetupRateWarning({
        quotedRate: 0.006,
        offeredRate: 0.00631,
        marketRate: null,
      })?.type,
    ).toBe("RateRaised");
  });

  it("accepts an offered rate up to 5% above the quote without a market rate", () => {
    expect(
      getSwapSetupRateWarning({
        quotedRate: 0.006,
        offeredRate: 0.0063,
        marketRate: null,
      }),
    ).toBeNull();
  });

  it("warns if there is no quote", () => {
    expect(
      getSwapSetupRateWarning({
        quotedRate: null,
        offeredRate: 0.006,
        marketRate: MARKET,
      })?.type,
    ).toBe("NoQuote");
  });
});
