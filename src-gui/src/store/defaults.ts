import { Network, Blockchain } from "./types";

export const DEFAULT_RENDEZVOUS_POINTS = [
  "/dns4/discover.unstoppableswap.net/tcp/8888/p2p/12D3KooWA6cnqJpVnreBVnoro8midDL9Lpzmg8oJPoAGi7YYaamE",
  "/dns4/discover2.unstoppableswap.net/tcp/8888/p2p/12D3KooWGRvf7qVQDrNR5nfYD6rKrbgeTi9x8RrbdxbmsPvxL4mw",
  "/dns4/darkness.su/tcp/8888/p2p/12D3KooWFQAgVVS9t9UgL6v1sLprJVM7am5hFK7vy9iBCCoCBYmU",
  "/dns4/eigen.center/tcp/8888/p2p/12D3KooWS5RaYJt4ANKMH4zczGVhNcw5W214e2DDYXnjs5Mx5zAT",
  "/dns4/swapanarchy.cfd/tcp/8888/p2p/12D3KooWRtyVpmyvwzPYXuWyakFbRKhyXGrjhq6tP7RrBofpgQGp",
  "/dns4/rendezvous.observer/tcp/8888/p2p/12D3KooWMjceGXrYuGuDMGrfmJxALnSDbK4km6s1i1sJEgDTgGQa",
  "/dns4/aswap.click/tcp/8888/p2p/12D3KooWQzW52mdsLHTMu1EPiz3APumG6vGwpCuyy494MAQoEa5X",
  "/dns4/getxmr.st/tcp/8888/p2p/12D3KooWHHwiz6WDThPT8cEurstomg3kDSxzL2L8pwxfyX2fpxVk",
];

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
      "tcp://electrum.eigenwallet.org:22293",
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
