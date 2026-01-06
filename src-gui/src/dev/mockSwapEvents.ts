import {
  BidQuote,
  MoneroAddressPool,
  QuoteWithAddress,
  TauriSwapProgressEvent,
} from "models/tauriModel";

// Mock transaction IDs
const MOCK_BTC_LOCK_TXID =
  "f4184fc596403b9d638783cf57adfe4c75c605f6356fbc91338530e9831e9e16";
const MOCK_XMR_LOCK_TXID =
  "a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8";
const MOCK_XMR_REDEEM_TXID =
  "b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9";
const MOCK_BTC_CANCEL_TXID =
  "c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0";
const MOCK_BTC_REFUND_TXID =
  "d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1";
const MOCK_BTC_EARLY_REFUND_TXID =
  "e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2";
const MOCK_BTC_PARTIAL_REFUND_TXID =
  "f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3";
const MOCK_BTC_AMNESTY_TXID =
  "a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4";
const MOCK_BTC_REFUND_BURN_TXID =
  "b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5";
const MOCK_BTC_FINAL_AMNESTY_TXID =
  "c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";

// Mock addresses
const MOCK_BTC_DEPOSIT_ADDRESS = "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq";
const MOCK_XMR_ADDRESS =
  "888tNkZrPN6JsEgekjMnABU4TBzc2Dt29EPAvkRxbANsAnjyPbb3iQ1YBRk1UXcdRsiKc9dhwMVgN5S9cQUiyoogDavup3H";

export const MOCK_SWAP_ID = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";

const MOCK_QUOTE: BidQuote = {
  price: 0.007,
  min_quantity: 10_000_000,
  max_quantity: 100_000_000,
};

const MOCK_QUOTE_WITH_ADDRESS: QuoteWithAddress = {
  multiaddr: "/ip4/127.0.0.1/tcp/9939",
  peer_id: "12D3KooWCdMKjesXMJz1SiZ7HgotrxuqhQJbP5sgBm2BwP1cqThi",
  quote: MOCK_QUOTE,
  version: "0.13.0",
};

const MOCK_RECEIVE_POOL: MoneroAddressPool = [
  { address: MOCK_XMR_ADDRESS, percentage: 100, label: "Main" },
];

const XMR_TARGET_CONFIRMATIONS = 10;

// Base scenario: swap start -> XMR locked (10 confirmations)
const baseScenario: TauriSwapProgressEvent[] = [
  { type: "ReceivedQuote", content: MOCK_QUOTE },
  {
    type: "WaitingForBtcDeposit",
    content: {
      deposit_address: MOCK_BTC_DEPOSIT_ADDRESS,
      max_giveable: 0,
      min_bitcoin_lock_tx_fee: 1000,
      known_quotes: [MOCK_QUOTE_WITH_ADDRESS],
    },
  },
  { type: "SwapSetupInflight", content: { btc_lock_amount: 50_000_000 } },
  { type: "RetrievingMoneroBlockheight" },
  { type: "BtcLockPublishInflight" },
  // BTC lock confirmations: 0, 1, 2
  { type: "BtcLockTxInMempool", content: { btc_lock_txid: MOCK_BTC_LOCK_TXID, btc_lock_confirmations: 0 } },
  { type: "BtcLockTxInMempool", content: { btc_lock_txid: MOCK_BTC_LOCK_TXID, btc_lock_confirmations: 1 } },
  { type: "BtcLockTxInMempool", content: { btc_lock_txid: MOCK_BTC_LOCK_TXID, btc_lock_confirmations: 2 } },
  { type: "VerifyingXmrLockTx", content: { xmr_lock_txid: MOCK_XMR_LOCK_TXID } },
  // XMR lock confirmations: 0 through 10
  ...Array.from({ length: XMR_TARGET_CONFIRMATIONS + 1 }, (_, i) => ({
    type: "XmrLockTxInMempool" as const,
    content: {
      xmr_lock_txid: MOCK_XMR_LOCK_TXID,
      xmr_lock_tx_confirmations: i,
      xmr_lock_tx_target_confirmations: XMR_TARGET_CONFIRMATIONS,
    },
  })),
];

const happyPath: TauriSwapProgressEvent[] = [
  ...baseScenario,
  { type: "PreflightEncSig" },
  { type: "InflightEncSig" },
  { type: "EncryptedSignatureSent" },
  { type: "RedeemingMonero" },
  {
    type: "XmrRedeemInMempool",
    content: { xmr_redeem_txids: [MOCK_XMR_REDEEM_TXID], xmr_receive_pool: MOCK_RECEIVE_POOL },
  },
  { type: "Released" },
];

const cooperativeRedeem: TauriSwapProgressEvent[] = [
  ...baseScenario,
  { type: "AttemptingCooperativeRedeem" },
  { type: "CooperativeRedeemAccepted" },
  { type: "RedeemingMonero" },
  {
    type: "XmrRedeemInMempool",
    content: { xmr_redeem_txids: [MOCK_XMR_REDEEM_TXID], xmr_receive_pool: MOCK_RECEIVE_POOL },
  },
  { type: "Released" },
];

const cooperativeRedeemRejected: TauriSwapProgressEvent[] = [
  ...baseScenario,
  { type: "AttemptingCooperativeRedeem" },
  { type: "CooperativeRedeemRejected", content: { reason: "Peer offline" } },
  { type: "WaitingForCancelTimelockExpiration" },
  { type: "CancelTimelockExpired" },
  { type: "BtcCancelled", content: { btc_cancel_txid: MOCK_BTC_CANCEL_TXID } },
  { type: "BtcRefundPublished", content: { btc_refund_txid: MOCK_BTC_REFUND_TXID } },
  { type: "BtcRefunded", content: { btc_refund_txid: MOCK_BTC_REFUND_TXID } },
  { type: "Released" },
];

const earlyRefund: TauriSwapProgressEvent[] = [
  ...baseScenario,
  { type: "BtcEarlyRefundPublished", content: { btc_early_refund_txid: MOCK_BTC_EARLY_REFUND_TXID } },
  { type: "BtcEarlyRefunded", content: { btc_early_refund_txid: MOCK_BTC_EARLY_REFUND_TXID } },
  { type: "Released" },
];

const partialRefundWithAmnesty: TauriSwapProgressEvent[] = [
  ...baseScenario,
  { type: "WaitingForCancelTimelockExpiration" },
  { type: "CancelTimelockExpired" },
  { type: "BtcCancelled", content: { btc_cancel_txid: MOCK_BTC_CANCEL_TXID } },
  {
    type: "BtcPartialRefundPublished",
    content: { btc_partial_refund_txid: MOCK_BTC_PARTIAL_REFUND_TXID, has_amnesty_signature: true },
  },
  {
    type: "BtcPartiallyRefunded",
    content: { btc_partial_refund_txid: MOCK_BTC_PARTIAL_REFUND_TXID, has_amnesty_signature: true },
  },
  { type: "BtcAmnestyPublished", content: { btc_amnesty_txid: MOCK_BTC_AMNESTY_TXID } },
  { type: "BtcAmnestyReceived", content: { btc_amnesty_txid: MOCK_BTC_AMNESTY_TXID } },
  { type: "Released" },
];

const partialRefundWithBurn: TauriSwapProgressEvent[] = [
  ...baseScenario,
  { type: "WaitingForCancelTimelockExpiration" },
  { type: "CancelTimelockExpired" },
  { type: "BtcCancelled", content: { btc_cancel_txid: MOCK_BTC_CANCEL_TXID } },
  {
    type: "BtcPartialRefundPublished",
    content: { btc_partial_refund_txid: MOCK_BTC_PARTIAL_REFUND_TXID, has_amnesty_signature: false },
  },
  {
    type: "BtcPartiallyRefunded",
    content: { btc_partial_refund_txid: MOCK_BTC_PARTIAL_REFUND_TXID, has_amnesty_signature: false },
  },
  { type: "BtcRefundBurnPublished", content: { btc_refund_burn_txid: MOCK_BTC_REFUND_BURN_TXID } },
  { type: "BtcRefundBurnt", content: { btc_refund_burn_txid: MOCK_BTC_REFUND_BURN_TXID } },
  { type: "Released" },
];

const partialRefundWithBurnAndFinalAmnesty: TauriSwapProgressEvent[] = [
  ...baseScenario,
  { type: "WaitingForCancelTimelockExpiration" },
  { type: "CancelTimelockExpired" },
  { type: "BtcCancelled", content: { btc_cancel_txid: MOCK_BTC_CANCEL_TXID } },
  {
    type: "BtcPartialRefundPublished",
    content: { btc_partial_refund_txid: MOCK_BTC_PARTIAL_REFUND_TXID, has_amnesty_signature: false },
  },
  {
    type: "BtcPartiallyRefunded",
    content: { btc_partial_refund_txid: MOCK_BTC_PARTIAL_REFUND_TXID, has_amnesty_signature: false },
  },
  { type: "BtcRefundBurnPublished", content: { btc_refund_burn_txid: MOCK_BTC_REFUND_BURN_TXID } },
  { type: "BtcRefundBurnt", content: { btc_refund_burn_txid: MOCK_BTC_REFUND_BURN_TXID } },
  { type: "BtcFinalAmnestyPublished", content: { btc_final_amnesty_txid: MOCK_BTC_FINAL_AMNESTY_TXID } },
  { type: "BtcFinalAmnestyConfirmed", content: { btc_final_amnesty_txid: MOCK_BTC_FINAL_AMNESTY_TXID } },
  { type: "Released" },
];

export const scenarios: Record<string, TauriSwapProgressEvent[]> = {
  happyPath,
  cooperativeRedeem,
  cooperativeRedeemRejected,
  earlyRefund,
  partialRefundWithAmnesty,
  partialRefundWithBurn,
  partialRefundWithBurnAndFinalAmnesty,
};

export type MockScenario = keyof typeof scenarios;
