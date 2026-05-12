import {
  ApprovalRequest,
  BidQuote,
  ExpiredTimelocks,
  GetSwapInfoResponse,
  LockBitcoinDetails,
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
const MOCK_BTC_WITHHOLD_TXID =
  "b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5";
const MOCK_BTC_MERCY_TXID =
  "c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";

// Mock timelock blocks for earnest deposit
const EARNEST_DEPOSIT_TARGET_BLOCKS = 3;

// Mock amounts for partial refund scenarios
const MOCK_BTC_LOCK_AMOUNT = 50_000_000; // 0.5 BTC
const MOCK_BTC_AMNESTY_AMOUNT = 1_000_000; // 0.01 BTC (2% of lock amount)

// Mock addresses
const MOCK_BTC_DEPOSIT_ADDRESS = "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq";
const MOCK_XMR_ADDRESS =
  "888tNkZrPN6JsEgekjMnABU4TBzc2Dt29EPAvkRxbANsAnjyPbb3iQ1YBRk1UXcdRsiKc9dhwMVgN5S9cQUiyoogDavup3H";

export const MOCK_SWAP_ID = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";

const MOCK_KNOWN_QUOTES: QuoteWithAddress[] = [
  {
    multiaddr: "/ip4/127.0.0.1/tcp/9939",
    peer_id: "12D3KooWCdMKjesXMJz1SiZ7HgotrxuqhQJbP5sgBm2BwP1cqThi",
    quote: {
      price: 0.0066,
      min_quantity: 10_000_000,
      max_quantity: 100_000_000,
      refund_policy: { type: "FullRefund" },
    },
    version: "4.0.0",
  },
  {
    multiaddr: "/ip4/192.168.1.50/tcp/9940",
    peer_id: "12D3KooWEyoppNCUzN3sX7atGxPHvqgZvUNQmKzz1mQvNfFhuqP9",
    quote: {
      price: 0.00662,
      min_quantity: 5_000_000,
      max_quantity: 200_000_000,
      refund_policy: {
        type: "PartialRefund",
        content: { anti_spam_deposit_ratio: 0.005 },
      },
    },
    version: "4.0.0",
  },
  {
    multiaddr: "/ip4/192.168.1.51/tcp/9941",
    peer_id: "12D3KooWQ1XmPttg1Ut5xD3mcJRoWQYEQ8C1BcvqtrdrDam6DCyn",
    quote: {
      price: 0.00664,
      min_quantity: 5_000_000,
      max_quantity: 200_000_000,
      refund_policy: {
        type: "PartialRefund",
        content: { anti_spam_deposit_ratio: 0.01 },
      },
    },
    version: "4.0.0",
  },
  {
    multiaddr: "/ip4/192.168.1.52/tcp/9942",
    peer_id: "12D3KooWKJPkR34byJ9Y5mN9wQ3hEG8mSN2yT8eC2cQFqL3x1V9u",
    quote: {
      price: 0.00666,
      min_quantity: 5_000_000,
      max_quantity: 200_000_000,
      refund_policy: {
        type: "PartialRefund",
        content: { anti_spam_deposit_ratio: 0.02 },
      },
    },
    version: "4.0.0",
  },
  {
    multiaddr: "/ip4/192.168.1.53/tcp/9943",
    peer_id: "12D3KooWJ1PiH4H9ceR85S8JQk8MtS5o1vBM2wFp1k3Wf4t8bX4p",
    quote: {
      price: 0.0067,
      min_quantity: 5_000_000,
      max_quantity: 200_000_000,
      refund_policy: {
        type: "PartialRefund",
        content: { anti_spam_deposit_ratio: 0.05 },
      },
    },
    version: "4.0.0",
  },
  {
    multiaddr: "/ip4/192.168.1.54/tcp/9944",
    peer_id: "12D3KooWH1TY76YnLwQe3F6uq5WnPaW9DdK7RzWQ6xQG9Sm4F1no",
    quote: {
      price: 0.00674,
      min_quantity: 5_000_000,
      max_quantity: 200_000_000,
      refund_policy: {
        type: "PartialRefund",
        content: { anti_spam_deposit_ratio: 0.1 },
      },
    },
    version: "4.0.0",
  },
  {
    multiaddr: "/ip4/192.168.1.55/tcp/9945",
    peer_id: "12D3KooWGW4QmZsfgz6R6eQ5PVbSYcT7B8WcV1P4DnK5LmN2Rt7y",
    quote: {
      price: 0.0068,
      min_quantity: 5_000_000,
      max_quantity: 200_000_000,
      refund_policy: {
        type: "PartialRefund",
        content: { anti_spam_deposit_ratio: 0.2 },
      },
    },
    version: "4.0.0",
  },
];

const MOCK_QUOTE: BidQuote = MOCK_KNOWN_QUOTES[0].quote;

const MOCK_RECEIVE_POOL: MoneroAddressPool = [
  { address: MOCK_XMR_ADDRESS, percentage: 1.0, label: "Main" },
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
      known_quotes: MOCK_KNOWN_QUOTES,
    },
  },
  { type: "SwapSetupInflight", content: { btc_lock_amount: 50_000_000 } },
  { type: "RetrievingMoneroBlockheight" },
  { type: "BtcLockPublishInflight" },
  // BTC lock confirmations: 0, 1, 2
  {
    type: "BtcLockTxInMempool",
    content: { btc_lock_txid: MOCK_BTC_LOCK_TXID, btc_lock_confirmations: 0 },
  },
  {
    type: "BtcLockTxInMempool",
    content: { btc_lock_txid: MOCK_BTC_LOCK_TXID, btc_lock_confirmations: 1 },
  },
  {
    type: "BtcLockTxInMempool",
    content: { btc_lock_txid: MOCK_BTC_LOCK_TXID, btc_lock_confirmations: 2 },
  },
  {
    type: "VerifyingXmrLockTx",
    content: { xmr_lock_txid: MOCK_XMR_LOCK_TXID },
  },
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
  { type: "ConstructingMoneroRedeem" },
  { type: "PublishingMoneroRedeem" },
  {
    type: "XmrRedeemPublished",
    content: {
      xmr_redeem_txids: [MOCK_XMR_REDEEM_TXID],
      xmr_receive_pool: MOCK_RECEIVE_POOL,
    },
  },
  {
    type: "XmrRedeemed",
    content: {
      xmr_redeem_txids: [MOCK_XMR_REDEEM_TXID],
      xmr_receive_pool: MOCK_RECEIVE_POOL,
    },
  },
  { type: "Released" },
];

const cooperativeRedeem: TauriSwapProgressEvent[] = [
  ...baseScenario,
  { type: "AttemptingCooperativeRedeem" },
  { type: "CooperativeRedeemAccepted" },
  { type: "ConstructingMoneroRedeem" },
  { type: "PublishingMoneroRedeem" },
  {
    type: "XmrRedeemPublished",
    content: {
      xmr_redeem_txids: [MOCK_XMR_REDEEM_TXID],
      xmr_receive_pool: MOCK_RECEIVE_POOL,
    },
  },
  {
    type: "XmrRedeemed",
    content: {
      xmr_redeem_txids: [MOCK_XMR_REDEEM_TXID],
      xmr_receive_pool: MOCK_RECEIVE_POOL,
    },
  },
  { type: "Released" },
];

const cooperativeRedeemRejected: TauriSwapProgressEvent[] = [
  ...baseScenario,
  { type: "AttemptingCooperativeRedeem" },
  { type: "CooperativeRedeemRejected", content: { reason: "Peer offline" } },
  { type: "WaitingForCancelTimelockExpiration" },
  { type: "CancelTimelockExpired" },
  {
    type: "BtcCancelPublished",
    content: {
      btc_cancel_txid: MOCK_BTC_CANCEL_TXID,
      btc_cancel_confirmations: 0,
      btc_cancel_target_confirmations: 1,
    },
  },
  { type: "BtcCancelled", content: { btc_cancel_txid: MOCK_BTC_CANCEL_TXID } },
  {
    type: "BtcRefundPublished",
    content: { btc_refund_txid: MOCK_BTC_REFUND_TXID },
  },
  { type: "BtcRefunded", content: { btc_refund_txid: MOCK_BTC_REFUND_TXID } },
  { type: "Released" },
];

const earlyRefund: TauriSwapProgressEvent[] = [
  ...baseScenario,
  {
    type: "BtcEarlyRefundPublished",
    content: { btc_early_refund_txid: MOCK_BTC_EARLY_REFUND_TXID },
  },
  {
    type: "BtcEarlyRefunded",
    content: { btc_early_refund_txid: MOCK_BTC_EARLY_REFUND_TXID },
  },
  { type: "Released" },
];

const partialRefundWithAmnesty: TauriSwapProgressEvent[] = [
  ...baseScenario,
  { type: "WaitingForCancelTimelockExpiration" },
  { type: "CancelTimelockExpired" },
  {
    type: "BtcCancelPublished",
    content: {
      btc_cancel_txid: MOCK_BTC_CANCEL_TXID,
      btc_cancel_confirmations: 0,
      btc_cancel_target_confirmations: 1,
    },
  },
  { type: "BtcCancelled", content: { btc_cancel_txid: MOCK_BTC_CANCEL_TXID } },
  {
    type: "BtcPartialRefundPublished",
    content: {
      btc_partial_refund_txid: MOCK_BTC_PARTIAL_REFUND_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
    },
  },
  {
    type: "BtcPartiallyRefunded",
    content: {
      btc_partial_refund_txid: MOCK_BTC_PARTIAL_REFUND_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
    },
  },
  // Waiting for earnest deposit timelock: 3/3, 2/3, 1/3, 0/3 blocks remaining
  ...Array.from({ length: EARNEST_DEPOSIT_TARGET_BLOCKS + 1 }, (_, i) => ({
    type: "WaitingForEarnestDepositTimelockExpiration" as const,
    content: {
      btc_partial_refund_txid: MOCK_BTC_PARTIAL_REFUND_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
      target_blocks: EARNEST_DEPOSIT_TARGET_BLOCKS,
      blocks_until_expiry: EARNEST_DEPOSIT_TARGET_BLOCKS - i,
    },
  })),
  {
    type: "BtcAmnestyPublished",
    content: {
      btc_amnesty_txid: MOCK_BTC_AMNESTY_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
    },
  },
  {
    type: "BtcAmnestyReceived",
    content: {
      btc_amnesty_txid: MOCK_BTC_AMNESTY_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
    },
  },
  { type: "Released" },
];

const partialRefundWithBurn: TauriSwapProgressEvent[] = [
  ...baseScenario,
  { type: "WaitingForCancelTimelockExpiration" },
  { type: "CancelTimelockExpired" },
  {
    type: "BtcCancelPublished",
    content: {
      btc_cancel_txid: MOCK_BTC_CANCEL_TXID,
      btc_cancel_confirmations: 0,
      btc_cancel_target_confirmations: 1,
    },
  },
  { type: "BtcCancelled", content: { btc_cancel_txid: MOCK_BTC_CANCEL_TXID } },
  {
    type: "BtcPartialRefundPublished",
    content: {
      btc_partial_refund_txid: MOCK_BTC_PARTIAL_REFUND_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
    },
  },
  {
    type: "BtcPartiallyRefunded",
    content: {
      btc_partial_refund_txid: MOCK_BTC_PARTIAL_REFUND_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
    },
  },
  // Waiting for earnest deposit timelock: 3/3, 2/3, 1/3, 0/3 blocks remaining
  ...Array.from({ length: EARNEST_DEPOSIT_TARGET_BLOCKS + 1 }, (_, i) => ({
    type: "WaitingForEarnestDepositTimelockExpiration" as const,
    content: {
      btc_partial_refund_txid: MOCK_BTC_PARTIAL_REFUND_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
      target_blocks: EARNEST_DEPOSIT_TARGET_BLOCKS,
      blocks_until_expiry: EARNEST_DEPOSIT_TARGET_BLOCKS - i,
    },
  })),
  {
    type: "BtcWithholdPublished",
    content: {
      btc_withhold_txid: MOCK_BTC_WITHHOLD_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
    },
  },
  {
    type: "BtcWithheld",
    content: {
      btc_withhold_txid: MOCK_BTC_WITHHOLD_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
    },
  },
  { type: "Released" },
];

const partialRefundWithWithholdAndMercy: TauriSwapProgressEvent[] = [
  ...baseScenario,
  { type: "WaitingForCancelTimelockExpiration" },
  { type: "CancelTimelockExpired" },
  {
    type: "BtcCancelPublished",
    content: {
      btc_cancel_txid: MOCK_BTC_CANCEL_TXID,
      btc_cancel_confirmations: 0,
      btc_cancel_target_confirmations: 1,
    },
  },
  { type: "BtcCancelled", content: { btc_cancel_txid: MOCK_BTC_CANCEL_TXID } },
  {
    type: "BtcPartialRefundPublished",
    content: {
      btc_partial_refund_txid: MOCK_BTC_PARTIAL_REFUND_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
    },
  },
  {
    type: "BtcPartiallyRefunded",
    content: {
      btc_partial_refund_txid: MOCK_BTC_PARTIAL_REFUND_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
    },
  },
  // Waiting for earnest deposit timelock: 3/3, 2/3, 1/3, 0/3 blocks remaining
  ...Array.from({ length: EARNEST_DEPOSIT_TARGET_BLOCKS + 1 }, (_, i) => ({
    type: "WaitingForEarnestDepositTimelockExpiration" as const,
    content: {
      btc_partial_refund_txid: MOCK_BTC_PARTIAL_REFUND_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
      target_blocks: EARNEST_DEPOSIT_TARGET_BLOCKS,
      blocks_until_expiry: EARNEST_DEPOSIT_TARGET_BLOCKS - i,
    },
  })),
  {
    type: "BtcWithholdPublished",
    content: {
      btc_withhold_txid: MOCK_BTC_WITHHOLD_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
    },
  },
  {
    type: "BtcWithheld",
    content: {
      btc_withhold_txid: MOCK_BTC_WITHHOLD_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
    },
  },
  {
    type: "BtcMercyPublished",
    content: {
      btc_mercy_txid: MOCK_BTC_MERCY_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
    },
  },
  {
    type: "BtcMercyConfirmed",
    content: {
      btc_mercy_txid: MOCK_BTC_MERCY_TXID,
      btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
      btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
    },
  },
  { type: "Released" },
];

export const scenarios: Record<string, TauriSwapProgressEvent[]> = {
  happyPath,
  cooperativeRedeem,
  cooperativeRedeemRejected,
  earlyRefund,
  partialRefundWithAmnesty,
  partialRefundWithBurn,
  partialRefundWithWithholdAndMercy,
};

export type MockScenario = keyof typeof scenarios;

// Mock LockBitcoin approval requests for testing confirmation screen

// Partial refund version (5% amnesty)
const MOCK_LOCK_BITCOIN_DETAILS_PARTIAL: LockBitcoinDetails = {
  btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
  btc_network_fee: 5000,
  xmr_receive_amount: 7_000_000_000_000, // 7 XMR in piconeros
  monero_receive_pool: MOCK_RECEIVE_POOL,
  swap_id: MOCK_SWAP_ID,
  btc_amnesty_amount: MOCK_BTC_AMNESTY_AMOUNT,
  has_full_refund_signature: false,
};

// Full refund version (no amnesty)
const MOCK_LOCK_BITCOIN_DETAILS_FULL: LockBitcoinDetails = {
  btc_lock_amount: MOCK_BTC_LOCK_AMOUNT,
  btc_network_fee: 5000,
  xmr_receive_amount: 7_000_000_000_000, // 7 XMR in piconeros
  monero_receive_pool: MOCK_RECEIVE_POOL,
  swap_id: MOCK_SWAP_ID,
  btc_amnesty_amount: 0,
  has_full_refund_signature: true,
};

const PARTIAL_REFUND_SCENARIOS: MockScenario[] = [
  "partialRefundWithAmnesty",
  "partialRefundWithBurn",
  "partialRefundWithWithholdAndMercy",
];

export function isPartialRefundScenario(scenario: MockScenario): boolean {
  return PARTIAL_REFUND_SCENARIOS.includes(scenario);
}

// --- Mock SwapStatusAlert data (3 zones) ---

const MOCK_ALERT_SWAP_IDS = [
  "mock-alert-0000-0000-0000-000000000001",
  "mock-alert-0000-0000-0000-000000000002",
  "mock-alert-0000-0000-0000-000000000003",
] as const;

const MOCK_SELLER = {
  peer_id: "12D3KooWCdMKjesXMJz1SiZ7HgotrxuqhQJbP5sgBm2BwP1cqThi",
  addresses: ["/ip4/127.0.0.1/tcp/9939"],
};

function makeMockSwapInfo(
  swapId: string,
  stateName: string,
): GetSwapInfoResponse {
  return {
    swap_id: swapId,
    seller: MOCK_SELLER,
    completed: false,
    start_date: "2026-01-01 12:00:00.000000 +00:00:00",
    state_name: stateName,
    xmr_amount: 7.5,
    btc_amount: 0.05,
    tx_lock_id: MOCK_BTC_LOCK_TXID,
    tx_cancel_fee: 1000,
    tx_refund_fee: 1000,
    tx_lock_fee: 1000,
    btc_refund_address: MOCK_BTC_DEPOSIT_ADDRESS,
    cancel_timelock: 24,
    punish_timelock: 144,
    monero_receive_pool: [
      { address: MOCK_XMR_ADDRESS, percentage: 100, label: "Main" },
    ],
  };
}

const MOCK_ALERT_TIMELOCKS: [string, ExpiredTimelocks][] = [
  [MOCK_ALERT_SWAP_IDS[0], { type: "None", content: { blocks_left: 20 } }],
  [MOCK_ALERT_SWAP_IDS[1], { type: "Cancel", content: { blocks_left: 100 } }],
  [MOCK_ALERT_SWAP_IDS[2], { type: "Punish" }],
];

export function getMockAlertData(): {
  swapInfos: GetSwapInfoResponse[];
  timelocks: [string, ExpiredTimelocks][];
} {
  return {
    swapInfos: MOCK_ALERT_SWAP_IDS.map((id) =>
      makeMockSwapInfo(id, "btc is locked"),
    ),
    timelocks: MOCK_ALERT_TIMELOCKS,
  };
}

export function getMockAlertCleanupData(): GetSwapInfoResponse[] {
  return MOCK_ALERT_SWAP_IDS.map((id) =>
    makeMockSwapInfo(id, "safely aborted"),
  );
}

export function getMockLockBitcoinApproval(
  scenario: MockScenario | null,
): ApprovalRequest {
  const isPartial = scenario !== null && isPartialRefundScenario(scenario);
  return {
    request_id: "00000000-0000-0000-0000-000000000001",
    request: {
      type: "LockBitcoin",
      content: isPartial
        ? MOCK_LOCK_BITCOIN_DETAILS_PARTIAL
        : MOCK_LOCK_BITCOIN_DETAILS_FULL,
    },
    request_status: {
      state: "Pending",
      content: {
        expiration_ts: Number.MAX_SAFE_INTEGER,
      },
    },
  };
}
