# **eigenwallet**

## Abstract

**_eigenwallet_** is an ambitious community effort to build the Monero wallet
for the future.

Previously known as _UnstoppableSwap_ ([Why rename?](rename.html)).

---

![eigenwallet app preview](imgs/eigenwallet.png)

<div style="text-align: center; margin: 1rem 0;">
  <a href="download.html" style="display: inline-flex; align-items: center; justify-content: space-around; width: 15rem; min-width: 200px; padding: 0.75rem 2rem; font-size: 1.4rem; font-weight: 600; color: hsl(0, 100%, 33%); background: transparent; border: 2.27px solid hsl(0, 100%, 33%); text-decoration: none; letter-spacing: 0.02em; transition: all 0.25s ease;" onmouseover="this.style.background='hsl(0, 100%, 33%)'; this.style.color='hsl(210, 20%, 98%)'; this.querySelector('svg').style.stroke='hsl(210, 20%, 98%)';" onmouseout="this.style.background='transparent'; this.style.color='hsl(0, 100%, 33%)'; this.querySelector('svg').style.stroke='hsl(0, 100%, 33%)';">
    <span style="text-align: center">Download</span>
    <svg width="24" height="24" viewBox="0 0 100 125" fill="none" xmlns="http://www.w3.org/2000/svg" stroke-width="3.25px" stroke-linecap="round" stroke-linejoin="round" style="stroke: hsl(0, 100%, 33%); transition: stroke 0.25s ease;"><path d="M40.4051 79.4303L40.4451 79.5003L40.4851 79.4803L40.4051 79.4303Z" stroke-linejoin="round"/><path d="M97.9351 85.7602L88.5651 90.4502L82.9651 93.2502L77.9351 95.7602L68.5651 90.3502L73.5951 87.8402L88.5651 80.3502L97.9351 85.7602Z" stroke-linejoin="round"/><path d="M97.9351 85.7603V112.64L77.9351 122.64V95.7603L82.9651 93.2503L88.5651 90.4502L97.9351 85.7603Z" stroke-linejoin="round"/><path d="M77.9351 95.7602V122.64L2.96509 79.3602V52.4802L12.3351 57.8902V74.0202L68.5651 106.48V90.3502L77.9351 95.7602Z" stroke-linejoin="round"/><path d="M24.4151 51.8503L22.9651 52.5803L12.3351 57.8903L2.96509 52.4803L19.8851 44.0203L22.9651 49.3403L24.4151 51.8503Z" stroke-linejoin="round"/><path d="M31.6651 64.3502L22.9651 68.7102L12.3351 74.0202V57.8902L22.9651 52.5802L24.4151 51.8502L31.6651 64.3502Z" stroke-linejoin="round"/><path d="M73.5951 87.8402L68.5651 90.3502V106.48L12.3351 74.0202L22.9651 68.7102L31.6651 64.3502L40.4051 79.4302L40.4451 79.5002L40.4851 79.4802H40.4951L40.5651 79.4402L50.4651 74.4902L73.5951 87.8402Z" stroke-linejoin="round"/><path d="M83.8751 56.1403L60.4451 69.5003L50.4651 74.4903L40.5651 79.4403L49.8551 74.1303L59.8351 68.4403L63.8751 66.1403L83.8751 56.1403Z" stroke-linejoin="round"/><path d="M83.8751 56.1403L63.8751 66.1403L57.3151 54.5603L65.1351 50.6503L77.3151 44.5603L83.8751 56.1403Z" stroke-linejoin="round"/><path d="M65.1351 7.69031V50.6503L57.3151 54.5603L53.1551 56.9303L45.1351 61.5003V17.6903L55.7651 12.3803L65.1351 7.69031Z" stroke-linejoin="round"/><path d="M65.1351 7.69027L55.7651 12.3803L45.1351 17.6903L35.7651 12.2803L55.7651 2.28027L65.1351 7.69027Z" stroke-linejoin="round"/><path d="M63.8751 66.1403L59.8351 68.4403L49.8551 74.1303L40.5651 79.4403L40.4951 79.4803H40.4851L40.4051 79.4303L31.6651 64.3503L24.4151 51.8503L22.9651 49.3403L19.8851 44.0203L17.0251 39.0903L23.5851 35.0803L30.3451 46.7403L31.3051 48.4003L32.3351 50.1703L35.7651 56.0903V12.2803L45.1351 17.6903V61.5003L53.1551 56.9303L57.3151 54.5603L63.8751 66.1403Z" stroke-linejoin="round"/><path d="M35.7651 28.9902V56.0902L32.3351 50.1702L31.3051 48.4002L30.3451 46.7402L23.5851 35.0802L35.7651 28.9902Z" stroke-linejoin="round"/></svg>
  </a>
</div>

## What?

**_eigenwallet_** is an ambitious effort to combine the frontier of crypto
privacy technology in a neat user-friendly packaging and make it accessible to
all.

- _eigenwallet_ is a self-custody Monero and Bitcoin wallet
- _eigenwallet_ is a battle-tested ($>3$ years) Monero $\leftrightarrow$ Bitcoin
  atomic swap implementation (called [_eigenswap_](eigenswap.html))
- _eigenwallet_ provides network privacy with its first-class Tor support (using
  arti^[9])
- _eigenwallet_ is platform-agnostic and works on Windows, Linux, macOS and will work on iOS and
  Android
- _eigenwallets_ will form a peer-to-peer network between its users which we call the
  [_eigenweb_](eigenweb.html).
- _eigenwallet_ will magically sync itself across different devices using a protocol
  called _eigensync_.

**_eigenwallet_** shares a tech stack with upcoming Monero tech (Rust + Libp2p).
It's perfectly positioned to integrate these in the future:

- _eigenwallet_ will be a Monero node (using cuprate^[10])
- _eigenwallet_ will bring its users access to liquidity pools such as Serai DEX^[12]

## Why "_eigenwallet_"?

- "eigen" means "to own" in german, emphasizing the self-ownership and
  self-custody you hold over your funds when using _eigenwallet_
- "eigen" is a prefix used for mathematical concepts (like eigenvalues and
  eigenvectors). Crypto and math are intrinstrically intertwined
- we use an algorithm called _eigentrust_ to build a decentralized trust system
  and assign a local trust score to each _peer_
- we think it sounds cool

## Why is this important?

To understand what we are building (and why) we must hold a few truths to be
non-negotiable.

**(1)** The primary use case for crypto is to be:

- peer-to-peer electronic cash^[1]
- a system to store, hold and transfer value irrespective of borders and
  governments

**(2)** Bitcoin is the most prominent attempt at establishing such a system.
Having gained widespread acceptance, it is here to stay for a while. It will
continue to be acquirable to most people on this planet for the foreseeable
future.

**(3)** Privacy is the most important aspect of any peer-to-peer electronic cash
system. Without it fungibility cannot be achieved. Bitcoin's public ledger
severely lacks in this regard.

**(4)** Monero is currently the best attempt at a better system. It is the best
choice because it:

- offers reasonably private transactions through ring signatures and
  bulletproofs^[2]
- enforces non-transparent uniform transactions
- has the largest user base of any privacy coin (by far), giving it the largest
  possible anonymity set^[4]
- has gained widespread adoption in circles where privacy is paramount^[5]
- has been chosen most valuable by the market^[8]
- has shown to be eager and willing to adopt new technology (eg FCMP++)^[3]

**(5)** Monero is under constant attack. It lives under the constant threat of
delistings. It is not unreasonable to assume that it will be banned from most
exchanges within the next 3-5 years^[6].

**(6)** We cannot rely on centralized exchanges. Governments will continue to
crack down against Monero^[7]. Centralized entities will fold under pressure
because they do not want to go to prison, and they need to stay profitable. This
will impact the ability to onboard new users to the network if the community
does not develop a reliable onramp that does not rely on the continued
functioning of these institutions.

Therefore any onramp **must**:

- be non-custodial (meaning users do not give up access to their private keys)
- be censorship resistant (meaning it does not rely on a central authority)
- provide radical privacy (both on the protocol and network level)
- have high liquidity (comparable to centralized exchanges)

The goal of _eigenwallet_ is to build exactly that onramp. In the process we
will be building a great wallet as well.

## References

[1]: Nakamoto, Satoshi. "Bitcoin: A Peer-to-Peer Electronic Cash System."
https://bitcoin.org/bitcoin.pdf. October 31, 2008.

[2]: koe, Kurt M. Alonso, Sarang Noether. "Zero to Monero: Second Edition - A
Technical Guide to a Private Digital Currency; for Beginners, Amateurs, and
Experts." https://www.getmonero.org/library/Zero-to-Monero-2-0-0.pdf. April
4, 2020.

[3]: kayabaNerve. 'Full-Chain Membership Proofs + Spend Authorization +
Linkability.'
<a href="https://gist.github.com/kayabaNerve/0e1f7719e5797c826b87249f21ab6f86" style="line-break: anywhere">https://gist.github.com/kayabaNerve/0e1f7719e5797c826b87249f21ab6f86</a>. _"This
proposes an extension to FCMPs to make them a drop-in replacement for the
existing CLSAG. In order to be such a replacement, the proof must handle
membership (inherent to FCMPs), spend authorization, and linkability."_

[4]: BitInfoCharts. 'Monero, Zcash Transactions historical chart - Number of
transactions in blockchain per day.'
https://bitinfocharts.com/comparison/transactions-xmr-zec.html. Historical
transaction data demonstrating Monero's significantly higher daily transaction
volume compared to other privacy-focused cryptocurrencies.

[5]: Chainalysis Team. "Darknet market and fraud shop BTC revenues decline amid
years-long international law enforcement disruption."
https://www.chainalysis.com/blog/darknet-markets-2025/. May 16, 2025. _"As
international authorities have disrupted DNMs large and small in the last few
years, cybercriminals and drug dealers have learned firsthand the consequences
of running BTC-accepting DNMs given the currency's inherent transparency. Many
operators have since moved to accepting only Monero (XMR), a privacy coin with
features designed to boost anonymity and reduce traceability."_

[6]: Evidence of ongoing exchange delistings includes: OKX delisting Monero,
Zcash, and Dash
(https://cointelegraph.com/news/report-okex-delisting-monero-dash-privacy-cryptos-over-fatf-demands),
Upbit delisting privacy coins
(https://cointelegraph.com/news/upbit-exchange-delists-privacy-coins-due-to-money-laundering-concerns),
BitBay delisting Monero
(https://de.cointelegraph.com/news/bitbay-crypto-exchange-to-delist-monero-due-to-money-laundering-concerns),
Bittrex delisting privacy coins
(https://www.coindesk.com/markets/2021/01/01/bittrex-to-delist-privacy-coins-monero-dash-and-zcash),
Kraken restricting Monero for UK customers
(https://cointelegraph.com/news/kraken-to-delist-monero-for-uk-customers-by-the-end-of-november),
Huobi delisting privacy coins
(https://cointelegraph.com/news/huobi-to-delist-monero-and-other-privacy-coins-citing-regulatory-pressures),
and Binance temporary delisting threats
(https://cointelegraph.com/news/monero-drops-multi-month-lows-binance-delisting).
https://support.kraken.com/articles/support-for-monero-xmr-in-europe.

[7]: Regulation (EU) 2024/1624 of the European Parliament and of the Council of
31 May 2024 on the prevention of the use of the financial system for the
purposes of money laundering or terrorist financing.
https://eur-lex.europa.eu/eli/reg/2024/1624/oj/eng. This comprehensive EU
regulation establishes strict anti-money laundering and counter-terrorist
financing requirements that significantly impact the regulatory environment for
privacy-focused cryptocurrencies and the exchanges that handle them.

[8]: BitInfoCharts. 'Monero, Zcash Market Capitalization historical chart.'

https://bitinfocharts.com/comparison/marketcap-xmr-zec.html. Market
capitalization data demonstrates Monero's significantly higher market valuation
compared to other major privacy-focused cryptocurrencies, indicating stronger
market confidence and adoption.

[9]: The Tor Project. "Arti: A Tor implementation in Rust."
https://tpo.pages.torproject.net/core/arti/about/. _"Arti is designed from the
ground up to work as a modular, embeddable library that other applications can
use."_

[10]: Cuprate. "Cuprate: An upcoming experimental, modern, and secure Monero
node." https://cuprate.org/. _"Discover the documentation of our Rust-written
project, as well as documentation on monerod and the Monero protocol."_

[11]: grease-xmr. "Payment channels for Monero."
https://github.com/grease-xmr/grease.

[12]: serai-dex. "Serai DEX." https://github.com/serai-dex/serai.
