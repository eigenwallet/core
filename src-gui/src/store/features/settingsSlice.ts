import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { Theme } from "renderer/components/theme";
import { DEFAULT_NODES, DEFAULT_RENDEZVOUS_POINTS } from "../defaults";
import { Network, Blockchain } from "../types";

// false = user hasn't selected yet (show dialog)
// 0 = user explicitly selected no tip
export type DonateToDevelopmentTip = false | 0 | 0.005 | 0.012 | 0.02;

// Options shown in the UI (excludes false since that means "not selected yet")
export const DONATE_TO_DEVELOPMENT_OPTIONS: Exclude<
  DonateToDevelopmentTip,
  false
>[] = [0, 0.005, 0.012, 0.02];

const MIN_TIME_BETWEEN_DEFAULT_NODES_APPLY = 14 * 24 * 60 * 60 * 1000; // 14 days

export interface SettingsState {
  /// This is an ordered list of node urls for each network and blockchain
  nodes: Record<Network, Record<Blockchain, string[]>>;
  /// Which theme to use
  theme: Theme;
  /// Whether to fetch fiat prices from the internet
  fetchFiatPrices: boolean;
  fiatCurrency: FiatCurrency;
  /// Whether to enable Tor for p2p connections
  enableTor: boolean;
  /// Whether to route Monero wallet traffic through Tor
  enableMoneroTor: boolean;
  /// Whether to use the Monero RPC pool for load balancing (true) or custom nodes (false)
  useMoneroRpcPool: boolean;
  userHasSeenIntroduction: boolean;
  /// List of rendezvous points
  rendezvousPoints: string[];
  /// Does the user want to donate parts of his swaps to funding the development
  /// of the project?
  donateToDevelopment: DonateToDevelopmentTip;
  /// Does the user want to withdraw Monero from Atomic Swaps to an external address?
  /// If set to 'internal', the funds will be sent to the internal wallet.
  moneroRedeemPolicy: RedeemPolicy;
  /// Does the user want to send Bitcoin refund from Atomic Swaps to an external address?
  /// If set to 'internal', the funds will be sent to the internal wallet.
  bitcoinRefundPolicy: RefundPolicy;
  /// The external Monero redeem address
  externalMoneroRedeemAddress: string;
  /// The external Bitcoin refund address
  externalBitcoinRefundAddress: string;
  /// UTC timestamp (in milliseconds) when default nodes were last applied
  lastAppliedDefaultNodes?: number | null;
}

export enum RedeemPolicy {
  Internal = "internal",
  External = "external",
}

export enum RefundPolicy {
  Internal = "internal",
  External = "external",
}

export enum FiatCurrency {
  Usd = "USD",
  Eur = "EUR",
  Gbp = "GBP",
  Chf = "CHF",
  Jpy = "JPY",
  // the following are copied from the coin gecko API and claude, not sure if they all work
  Aed = "AED",
  Ars = "ARS",
  Aud = "AUD",
  Bdt = "BDT",
  Bhd = "BHD",
  Bmd = "BMD",
  Brl = "BRL",
  Cad = "CAD",
  Clp = "CLP",
  Cny = "CNY",
  Czk = "CZK",
  Dkk = "DKK",
  Gel = "GEL",
  Hkd = "HKD",
  Huf = "HUF",
  Idr = "IDR",
  Ils = "ILS",
  Inr = "INR",
  Krw = "KRW",
  Kwd = "KWD",
  Lkr = "LKR",
  Mmk = "MMK",
  Mxn = "MXN",
  Myr = "MYR",
  Ngn = "NGN",
  Nok = "NOK",
  Nzd = "NZD",
  Php = "PHP",
  Pkr = "PKR",
  Pln = "PLN",
  Rub = "RUB",
  Sar = "SAR",
  Sek = "SEK",
  Sgd = "SGD",
  Thb = "THB",
  Try = "TRY",
  Twd = "TWD",
  Uah = "UAH",
  Ves = "VES",
  Vnd = "VND",
  Zar = "ZAR",
}

const initialState: SettingsState = {
  nodes: DEFAULT_NODES,
  theme: Theme.Dark,
  fetchFiatPrices: false,
  fiatCurrency: FiatCurrency.Usd,
  enableTor: true,
  enableMoneroTor: false, // Default to not routing Monero traffic through Tor
  useMoneroRpcPool: true, // Default to using RPC pool
  userHasSeenIntroduction: false,
  // TODO: Apply these regularly (like the default nodes)
  rendezvousPoints: DEFAULT_RENDEZVOUS_POINTS,
  donateToDevelopment: false, // Default to no donation
  moneroRedeemPolicy: RedeemPolicy.Internal,
  bitcoinRefundPolicy: RefundPolicy.Internal,
  externalMoneroRedeemAddress: "",
  externalBitcoinRefundAddress: "",
  lastAppliedDefaultNodes: null,
};

const alertsSlice = createSlice({
  name: "settings",
  initialState,
  reducers: {
    moveUpNode(
      slice,
      action: PayloadAction<{
        network: Network;
        type: Blockchain;
        node: string;
      }>,
    ) {
      const index = slice.nodes[action.payload.network][
        action.payload.type
      ].indexOf(action.payload.node);
      if (index > 0) {
        const temp =
          slice.nodes[action.payload.network][action.payload.type][index];
        slice.nodes[action.payload.network][action.payload.type][index] =
          slice.nodes[action.payload.network][action.payload.type][index - 1];
        slice.nodes[action.payload.network][action.payload.type][index - 1] =
          temp;
      }
    },
    setTheme(slice, action: PayloadAction<Theme>) {
      slice.theme = action.payload;
    },
    setFetchFiatPrices(slice, action: PayloadAction<boolean>) {
      slice.fetchFiatPrices = action.payload;
    },
    setFiatCurrency(slice, action: PayloadAction<FiatCurrency>) {
      slice.fiatCurrency = action.payload;
    },
    addRendezvousPoint(slice, action: PayloadAction<string>) {
      slice.rendezvousPoints.push(action.payload);
    },
    removeRendezvousPoint(slice, action: PayloadAction<string>) {
      slice.rendezvousPoints = slice.rendezvousPoints.filter(
        (point) => point !== action.payload,
      );
    },
    addNode(
      slice,
      action: PayloadAction<{
        network: Network;
        type: Blockchain;
        node: string;
      }>,
    ) {
      // Make sure the node is not already in the list
      if (
        slice.nodes[action.payload.network][action.payload.type].includes(
          action.payload.node,
        )
      ) {
        return;
      }
      // Add the node to the list
      slice.nodes[action.payload.network][action.payload.type].push(
        action.payload.node,
      );
    },
    removeNode(
      slice,
      action: PayloadAction<{
        network: Network;
        type: Blockchain;
        node: string;
      }>,
    ) {
      slice.nodes[action.payload.network][action.payload.type] = slice.nodes[
        action.payload.network
      ][action.payload.type].filter((node) => node !== action.payload.node);
    },
    setUserHasSeenIntroduction(slice, action: PayloadAction<boolean>) {
      slice.userHasSeenIntroduction = action.payload;
    },
    resetSettings(_) {
      return initialState;
    },
    setTorEnabled(slice, action: PayloadAction<boolean>) {
      slice.enableTor = action.payload;
    },
    setEnableMoneroTor(slice, action: PayloadAction<boolean>) {
      slice.enableMoneroTor = action.payload;
    },
    setUseMoneroRpcPool(slice, action: PayloadAction<boolean>) {
      slice.useMoneroRpcPool = action.payload;
    },
    setDonateToDevelopment(
      slice,
      action: PayloadAction<DonateToDevelopmentTip>,
    ) {
      slice.donateToDevelopment = action.payload;
    },
    setMoneroRedeemPolicy(slice, action: PayloadAction<RedeemPolicy>) {
      slice.moneroRedeemPolicy = action.payload;
    },
    setBitcoinRefundPolicy(slice, action: PayloadAction<RefundPolicy>) {
      slice.bitcoinRefundPolicy = action.payload;
    },
    setMoneroRedeemAddress(slice, action: PayloadAction<string>) {
      slice.externalMoneroRedeemAddress = action.payload;
    },
    setBitcoinRefundAddress(slice, action: PayloadAction<string>) {
      slice.externalBitcoinRefundAddress = action.payload;
    },
    applyDefaultNodes(
      slice,
      action: PayloadAction<{
        defaultNodes: Record<Network, Record<Blockchain, string[]>>;
        negativeNodesMainnet: string[];
        negativeNodesTestnet: string[];
      }>,
    ) {
      const now = Date.now();
      const twoWeeksInMs = 14 * 24 * 60 * 60 * 1000;

      // Check if we should apply defaults (first time or more than 2 weeks)
      if (
        slice.lastAppliedDefaultNodes == null ||
        now - slice.lastAppliedDefaultNodes >
          MIN_TIME_BETWEEN_DEFAULT_NODES_APPLY
      ) {
        // Remove negative nodes from mainnet
        slice.nodes[Network.Mainnet][Blockchain.Bitcoin] = slice.nodes[
          Network.Mainnet
        ][Blockchain.Bitcoin].filter(
          (node) => !action.payload.negativeNodesMainnet.includes(node),
        );

        // Remove negative nodes from testnet
        slice.nodes[Network.Testnet][Blockchain.Bitcoin] = slice.nodes[
          Network.Testnet
        ][Blockchain.Bitcoin].filter(
          (node) => !action.payload.negativeNodesTestnet.includes(node),
        );

        // Add new default nodes if they don't exist (mainnet)
        action.payload.defaultNodes[Network.Mainnet][
          Blockchain.Bitcoin
        ].forEach((node) => {
          if (
            !slice.nodes[Network.Mainnet][Blockchain.Bitcoin].includes(node)
          ) {
            slice.nodes[Network.Mainnet][Blockchain.Bitcoin].push(node);
          }
        });

        // Add new default nodes if they don't exist (testnet)
        action.payload.defaultNodes[Network.Testnet][
          Blockchain.Bitcoin
        ].forEach((node) => {
          if (
            !slice.nodes[Network.Testnet][Blockchain.Bitcoin].includes(node)
          ) {
            slice.nodes[Network.Testnet][Blockchain.Bitcoin].push(node);
          }
        });

        // Update the timestamp
        slice.lastAppliedDefaultNodes = now;
      }
    },
    /// Validates the donate to development tip setting.
    /// If the current tip is not in the valid options array, it will be replaced
    /// with the closest smaller valid option.
    /// false means "not yet selected" and is kept as-is
    validateDonateToDevelopmentTip(slice) {
      const currentTip = slice.donateToDevelopment;

      // false means "not yet selected" - keep it to show the dialog
      if (currentTip === false) {
        return;
      }

      // Check if current tip is a valid option
      if (DONATE_TO_DEVELOPMENT_OPTIONS.includes(currentTip)) {
        return;
      }

      // Invalid numeric tip - find closest smaller valid option
      const sorted = [...DONATE_TO_DEVELOPMENT_OPTIONS].sort((a, b) => b - a);
      const match = sorted.find((o) => o <= currentTip);

      // If no match was found, set to false to show the dialog and let the user choose explicitly
      slice.donateToDevelopment = match ?? false;
    },
  },
});

export const {
  moveUpNode,
  setTheme,
  addNode,
  removeNode,
  resetSettings,
  setFetchFiatPrices,
  setFiatCurrency,
  setTorEnabled,
  setEnableMoneroTor,
  setUseMoneroRpcPool,
  setUserHasSeenIntroduction,
  addRendezvousPoint,
  removeRendezvousPoint,
  setDonateToDevelopment,
  setMoneroRedeemPolicy,
  setBitcoinRefundPolicy,
  setMoneroRedeemAddress,
  setBitcoinRefundAddress,
  applyDefaultNodes,
  validateDonateToDevelopmentTip,
} = alertsSlice.actions;

export default alertsSlice.reducer;
