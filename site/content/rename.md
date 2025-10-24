## _UnstoppableSwap_ is now _Eigenwallet_

The _eigenwallet_ project was previously called _UnstoppableSwap_. Why the rename you might ask?

#### A trip down memory lane

The name _"UnstoppableSwap"_ was sworn into existance almost [three years](https://github.com/eigenwallet/unstoppableswap-site/commit/071a7435eb69a49d9c66bad968c637937e1173c0) ago. It comes from a time where atomic swaps were still in their infancy. This project was the only working implementation. It was just me ([_binarybaron_](https://github.com/binarybaron)) working on the project. There didn't even exist a proper UI back then, just hope, CLI commands and a bash script generator.

If you want see what the UX looked like back then, check out [this video](https://www.youtube.com/watch?v=ZQG50hJTgBA) some guy made back then.

Back then we were solely focused on getting Bitcoin $\leftrightarrow$ Monero atomic swaps to work. _"UnstoppableSwap"_ was descriptive, clear and to the point. The name was also in line with the naming conventions for Ethereum projects (like [Uniswap](https://app.uniswap.org/)). We just wanted to start building and had no interest in spending time thinking about a name.

Some time has passed since then. [_Einliterflasche_](https://github.com/Einliterflasche) joined me (with a lot more experience with Rust). We spend a few months building a [GUI using Electron](https://github.com/eigenwallet/unstoppableswap-gui). Then we scrapped that again and rebuilt the whole thing in pure Rust using [Tauri](https://v2.tauri.app/).
We worked for multiple months to remove our dependency on the heavy and clunky [monero-wallet-rpc](https://www.getmonero.org/resources/developer-guides/wallet-rpc.html) and replaced it with [native bindings](https://github.com/eigenwallet/core/tree/master/monero-sys) to the Monero C++ codebase. We integrated Tor natively into the project using [Arti](https://gitlab.torproject.org/tpo/core/arti) and gave every peer in the network a _hidden service_ with a `.onion` address.

[_Benedict_](https://github.com/b-enedict) and [Maksim](https://github.com/Matroskine) also joined the project to help us with building a great UI. They have truly helped us immensely.

We didn't do any marketing or publicity work. We had 10-100 hardcore users that were testing every single one of our releases and giving us feedback. Liquidity kept increasing and the software was getting more and more stable.

We could tell that most of our users were people who wanted to swap large amounts. They didn't feel safe using unreliable instant exchanges. They wanted to have full control over their funds. And we were offering them that.

![image](imgs/stat.png)

#### The new direction

Over the last two years, we have come to a realization:
> Building _just_ a working implementation for swaps is not enough. We want the masses to switch to DEXs. We need to make them as easy (if not easier) to use than using a CEX. Making them accessible to use is the _hardest_ part of building this. 

Atomic swaps cannot work in the same way as something like [ChangeNow](https://changenow.io/de). They offer strong security guarantees but they are also messy and complex. A swap takes time. They need stateful information saved in a local database. You need to handle Bitcoin refunds when things go wrong. You need reliable connections to Bitcoin and Monero blockchains. And you need some kind of trustless reputation system to avoid spam.

We are confident that, the only way to make this work is to:
> Build the best non-custodial cross-platform Monero wallet. A wallet that is recommended by the community to new users. A wallet that is loved by power-users. A wallet with first-class support for Atomic Swaps.

This is what we are building. We know where we are going, and we are fairly sure we know how to get there. It's less of a sprint and more of a marathon but we are in this for the long haul. We want this to exist, someone needs to build it, and we are going to be the ones to do it.

#### Signed proof

Below is a signed proof by me ([_binarybaron_](https://github.com/binarybaron)) that I willingly renamed the project and I am in control of the eigenwallet.org domain and the new GitHub organization.

```
-----BEGIN PGP SIGNED MESSAGE-----
Hash: SHA512

UnstoppableSwap has renamed to eigenwallet. UnstoppableSwap.net has moved to eigenwallet.org. The Github organization has moved from https://github.com/UnstoppableSwap/core to https://github.com/eigenwallet/core. Signed by binarybaron (core developer)
-----BEGIN PGP SIGNATURE-----

iHUEARYKAB0WIQQ1qETX9LVbxE4YD/GZt10+FHaibgUCaH30KAAKCRCZt10+FHai
bl6JAQCX4TJjn1MHuyDQ6jbi3h4eE9Iv8/D0VMC/wIfcfEoTcAD/c/9DHHxXuY0p
XpfUady82HlQvbhF6fGHs1mDDRC4Xgg=
=v8bE
-----END PGP SIGNATURE-----
```