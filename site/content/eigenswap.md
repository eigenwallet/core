## What is _eigenswap_?
- It's a Monero $\leftrightarrow$ Bitcoin atomic swap implementation written in Rust
- It has been battle-tested for $>3$ years. It has proven to be secure and reasonably reliable
- It's the primary way to onboard _eigenwallet_ users into the Monero ecosystem
- It allows swapping anything from 20€ to 500,000€ of Bitcoin into Monero,
  all without counterparty risk and without the need for pre-established trust
- We conservatively estimate it has secured > 80,000,000€ worth of Bitcoin $\leftrightarrow$ Monero swaps over its lifetime.

## How does it work?
- There a _makers_ and _takers_:
  - _Takers_ run a short running process (through the _eigenwallet_).
    They want to convert their Bitcoin into Monero.
  - _Makers_ run a long running process (we call the _asb_). They offer Monero for sale and in exchange they ask for Bitcoin.
  - _Makers_ run a Tor _hidden service_^[2] which hides their IP address. _Takers_ connect to that _hidden service_ to take them up on their offers.
- _Makers_ sell slightly above market price (usually 0.5% - 4% above market price). They compete with each other to offer the best exchange rate. Due to the strong incentive to profit off the arbitrage, there'll always be enough _makers_ to provide a enough liquidity and markups stay low.
- It implements the protocol described in a paper by Philipp Hoenisch and Lucas Soriano del Pino^[1]

[1]: Philipp Hoenisch and Lucas Soriano del Pino, _Atomic Swaps between Bitcoin and Monero_, arXiv:2101.12332, 2021. https://arxiv.org/abs/2101.12332
[2]: Wikipedia, _Tor (network) - Onion services_, https://en.wikipedia.org/wiki/Tor_(network)#Onion_services