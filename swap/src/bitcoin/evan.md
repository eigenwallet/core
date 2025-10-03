# What are we using bdk for?

Note: I will add more context to this document over the coming days.

### Syncing our wallet

- Because some of our users have large wallets with thousands of spks syncing can take a long time.
- In an attempt to (1) speed up syncing and (2) to have the ability to display some sort of progress indicator, we have written a wrapper around the bdk API ([wallet.rs#914-1134](https://github.com/eigenwallet/core/blob/9d42814726ac33fa5976d82c18793037426eb3fa/swap/src/bitcoin/wallet.rs#L914-L1134), [wallet.rs#2162-2329](https://github.com/eigenwallet/core/blob/9d42814726ac33fa5976d82c18793037426eb3fa/swap/src/bitcoin/wallet.rs#L2162-L2329)).
  - The `Wallet::chunked_sync_request` tries to split the spks of the wallet into reasonable chunks and returns a `Vec<SyncRequestBuilderFactory>`.
  - A `SyncRequestBuilderFactory` can be converted into a `bdk_core::spk_client::SyncRequestBuilder` which can then be actually used to sync the wallet. This is done by passing one of the chunks returned by `Wallet::chunked_sync_request` (`SyncRequestBuilderFactory`) to `Wallet::sync_with_custom_callback` which then calls the Electrum client and applies the update to the bdk wallet itself.
  - All of this is combined in the `Wallet::chunked_sync_with_callback` function which first calls `Wallet::chunked_sync_request` and then proceeds to call `Wallet::sync_with_custom_callback` for each of the chunks in parallel.
  - The `sync_ext` module contains some of abstractions such that we can pass in callbacks for receiving progress updates.

### Watching for the publication specific transactions

- In some cases we call `get_raw_transaction` directly in cases where we really need to make we know if the transaction is present in either the mempool or a block ([wallet.rs#1912-1975](https://github.com/eigenwallet/core/blob/9d42814726ac33fa5976d82c18793037426eb3fa/swap/src/bitcoin/wallet.rs#L1912-L1975)). Because the Electrum protocol does not differentiate clearly between a transaction not existing and between error we have to manually parse the returned error and attempt to extract the Bitcoin Core RPC error code ([wallet.rs#1942-1956](https://github.com/eigenwallet/core/blob/9d42814726ac33fa5976d82c18793037426eb3fa/swap/src/bitcoin/wallet.rs#L1942-L1956))

### Watch for the number of confirmations specific transactions

### Estimate fees for transactions

- We need to cache fee estimations because clients do fee estimations very regularly ([wallet.rs#289-367](https://github.com/eigenwallet/core/blob/3b701fe1c52314ddf1f59dece3584ce561f22e24/swap/src/bitcoin/wallet.rs#L289-L367))
- Some electrum server sporadically return -1 or 0 for fee estimations. Do work around this we also connect to the mempool.space API ([wallet.rs#2562-2618](https://github.com/eigenwallet/core/blob/3b701fe1c52314ddf1f59dece3584ce561f22e24/swap/src/bitcoin/wallet.rs#L2562-L2618))
- If either of them fail, we fallback to the other one. If both are successful, we use the higher one ([wallet.rs#1266-1319](https://github.com/eigenwallet/core/blob/3b701fe1c52314ddf1f59dece3584ce561f22e24/swap/src/bitcoin/wallet.rs#L1266-L1319))
- The caching was added in this [PR](https://github.com/eigenwallet/core/pull/411).

### Publish transactions

- When we decide to publish a transaction, it is important that we can be absolutely sure that it will propagate through the entire network.
- When we publish a transaction, we submit it at all of the known Electrum servers in parallel. The `electrum_pool` crate exposes a [`join_all(...)`](https://github.com/eigenwallet/core/blob/9d42814726ac33fa5976d82c18793037426eb3fa/electrum-pool/src/lib.rs#L330-L423) method which accepts a closure that is called for each of the Electrum servers. This is used in [`Wallet::broadcast`](https://github.com/eigenwallet/core/blob/9d42814726ac33fa5976d82c18793037426eb3fa/swap/src/bitcoin/wallet.rs#L714-L791). We mark a transaction as successfully published if at least one of the Electrum servers accepted it.

The bdk code has evolved over time and we have never found the time to properly refactor it; last time was when we switched from bdk <1.0.0 to bdk >=1.0.0. This was done in this [PR](https://github.com/eigenwallet/core/pull/180).

# Whats the issue?

- Requests are failing sporadically
- Its difficult to find out why requests are failing
- It is difficult to differentiate between network errors / protocol errors / ...
- It is a pain to work around the non-async API

# What is our goal?

- Improve reliability, reduce unexplainable sporadic failures
- Improve upon wallet abstractions and helper functions
  - Split wallet into smaller parts. Make the Bitcoin wallet code more maintable.
  - [This](https://github.com/eigenwallet/core/pull/530) pull request attempts to split the Bitcoin wallet (or at least its trait) into its crate.
  - A `ensure_broadcasted` that returns `Ok(txid)` if the transaction was broadcasted successfully or if the transaction was already present on the chain. Electrum will return a Bitcoin Core RPC error code if the transaction is rejected because it is already present on the chain.
  - ...
- Be generic over the transport such that we can use [arti](https://gitlab.torproject.org/tpo/core/arti) (tor implementation in rust)
- Possibly switch over to using Electrum subscriptions. It needs to provide the same guarantees regarding liveliness of returned data. We still need to have the ability to explictly request to fetch the latest state from the Electrum server. There are cases where we need to make a decision and we want to ensure we know if a certain transaction is present or not before we make a decision.

We want someone with experience to review the code and spot possible improvements.

# What is not as important?

- Performance (as in speed of requests) although it'd be nice to have
