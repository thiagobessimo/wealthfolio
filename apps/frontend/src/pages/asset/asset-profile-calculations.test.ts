import { describe, expect, it } from "vitest";

import { ActivityStatus, ActivityType, AssetKind } from "@/lib/constants";
import type { ActivityDetails, Asset, Quote } from "@/lib/types";

import {
  normalizeQuoteHistoryForDisplay,
  resolveBackendMarketQuoteFallback,
  resolveQuoteDisplayFactor,
  sumDisplayIncomeActivities,
} from "./asset-profile-calculations";

const quote = (overrides: Partial<Quote>): Quote => ({
  id: "quote-cty",
  assetId: "asset-cty-lse",
  timestamp: "2026-03-03T00:00:00.000Z",
  createdAt: "2026-03-03T00:00:00.000Z",
  dataSource: "MANUAL",
  open: 556.765,
  high: 570,
  low: 550,
  close: 565,
  adjclose: 565,
  volume: 1000,
  currency: "GBp",
  ...overrides,
});

const incomeActivity = (overrides: Partial<ActivityDetails>): ActivityDetails =>
  ({
    id: "income-1",
    activityType: ActivityType.DIVIDEND,
    status: ActivityStatus.POSTED,
    amount: "100",
    currency: "GBp",
    assetId: "asset-cty-lse",
    ...overrides,
  }) as ActivityDetails;

describe("asset profile calculations", () => {
  it("uses backend-normalized market quote fields for quote-unit assets", () => {
    const asset: Asset = {
      id: "asset-cty-lse",
      kind: AssetKind.INVESTMENT,
      quoteMode: "MARKET",
      quoteCcy: "GBp",
      displayMarketPrice: 5.65,
      displayMarketCurrency: "GBP",
      createdAt: "2026-03-03T12:00:00.000Z",
      updatedAt: "2026-03-03T12:00:00.000Z",
    };

    expect(
      resolveBackendMarketQuoteFallback({
        asset,
        instrumentCurrency: null,
        baseCurrency: "USD",
      }),
    ).toEqual({
      marketPrice: 5.65,
      currency: "GBP",
    });
  });

  it("normalizes quote history to the backend display price scale", () => {
    const factor = resolveQuoteDisplayFactor({
      quote: quote({ close: 565 }),
      displayCurrency: "GBP",
      marketPrice: 5.65,
    });

    expect(factor).toBeCloseTo(0.01);

    const [displayQuote] = normalizeQuoteHistoryForDisplay({
      quoteHistory: [quote({ close: 556.765, adjclose: 556.765 })],
      displayCurrency: "GBP",
      quoteDisplayFactor: factor,
    });

    expect(displayQuote.close).toBeCloseTo(5.56765);
    expect(displayQuote.adjclose).toBeCloseTo(5.56765);
    expect(displayQuote.currency).toBe("GBP");
  });

  it("normalizes quote-unit income fallback amounts to display currency", () => {
    expect(
      sumDisplayIncomeActivities({
        activities: [incomeActivity({ amount: "100", currency: "GBp" })],
        displayCurrency: "GBP",
        quoteDisplayFactor: 0.01,
      }),
    ).toBeCloseTo(1);
  });

  it("suppresses income fallback for currencies that cannot be displayed safely", () => {
    expect(
      sumDisplayIncomeActivities({
        activities: [incomeActivity({ amount: "100", currency: "EUR" })],
        displayCurrency: "GBP",
        quoteDisplayFactor: 0.01,
      }),
    ).toBeNull();
  });
});
