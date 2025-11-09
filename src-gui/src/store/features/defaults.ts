import { Network, Blockchain } from "./types";

// Known broken nodes to remove when applying defaults
export const NEGATIVE_NODES_MAINNET = [
  "tcp://electrum.blockstream.info:50001",
  "tcp://electrum.coinucopia.io:50001",
  "tcp://se-mma-crypto-payments-001.mullvad.net:50001",
  "tcp://electrum2.bluewallet.io:50777",
];

export const NEGATIVE_NODES_TESTNET = [
  "ssl://ax101.blockeng.ch:60002",
  "tcp://electrum.blockstream.info:60001",
  "tcp://blockstream.info:143",
  "ssl://testnet.qtornado.com:50002",
  "ssl://testnet.qtornado.com:51002",
  "tcp://testnet.qtornado.com:51001",
];

export const DEFAULT_NODES: Record<Network, Record<Blockchain, string[]>> = {
  [Network.Testnet]: {
    [Blockchain.Bitcoin]: [
      "ssl://blackie.c3-soft.com:57006",
      "ssl://v22019051929289916.bestsrv.de:50002",
      "tcp://v22019051929289916.bestsrv.de:50001",
      "ssl://electrum.blockstream.info:60002",
      "ssl://blockstream.info:993",
      "tcp://testnet.aranguren.org:51001",
      "ssl://testnet.aranguren.org:51002",
      "ssl://bitcoin.devmole.eu:5010",
      "tcp://bitcoin.devmole.eu:5000",
    ],
    [Blockchain.Monero]: [],
  },
  [Network.Mainnet]: {
    [Blockchain.Bitcoin]: [
      "ssl://electrum.blockstream.info:50002",
      "ssl://bitcoin.stackwallet.com:50002",
      "ssl://b.1209k.com:50002",
      "ssl://mainnet.foundationdevices.com:50002",
      "tcp://bitcoin.lu.ke:50001",
      "ssl://electrum.coinfinity.co:50002",
      "tcp://electrum1.bluewallet.io:50001",
      "tcp://electrum2.bluewallet.io:50001",
      "tcp://electrum3.bluewallet.io:50001",
      "ssl://btc-electrum.cakewallet.com:50002",
      "tcp://bitcoin.aranguren.org:50001",
    ],
    [Blockchain.Monero]: [],
  },
};
