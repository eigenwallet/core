## What is the _eigenweb_?

- It's a lightweight message relaying and buffering system
- It allows for asynchronous communication
- It assigns a local trust score to each _peer_ using the _eigentrust_
  system^[1]
- The amount of storage any given _eigenwallet_ allocates for data packets from
  a specific other _eigenwallet_ user depends on its _trust score_.

## What can the _eigenweb_ do?

- Alice starts an Bitcoin $\leftrightarrow$ Monero atomic swap with Bob. Alice
  locks her Monero. It takes a while for the Bitcoin transaction to confirm.
  Once the transactions confirms, Bob locks his Monero. Alice however has gone
  to get a coffee down the street. She is not online to receive messages.

  Bob stores the _transfer proof_ in the _eigenweb_. When Alice comes back she
  gets the _transfer proof_ from the _eigenweb_ and sends Bob the key to redeem
  the Bitcoin.

- Alice wants to swap her Bitcoin for Monero. She wants to minimize the
  probability of the other party not locking their Monero. This would force her
  to do a refund which will cost her around 1$ in fees.

  Alice has two friends (Bob and Charlie) that she trusts. Bob and Charlie have
  each already done three successful swaps respectively. Alice retrieves the
  _trust scores_ Bob and Charlie have assigned to the _makers_ whom they have
  swapped with. She chooses David as both Bob and Charlie have assigned a high
  trust score to him.

- Alice tries to swap her Bitcoin for Monero with Bob. Alice locks her Bitcoin and waits. Bob simply does not lock his Monero. Alice has to wait a few hours to then refund her Bitcoin. She creates a cryptographic proof which others can verify. The proof conclusively shows that Bob did not lock his Monero despite previously having committed to do so. Alice publishes the proof on the _eigenweb_. Other users will now know that Bob is not a trustworthy maker and avoid him in the future.

## How does _eigentrust_ work?

The _eigentrust_ algorithm operates under the basic principle of:

> "I extend trust to those who are trusted by people I already trust".

As more trust is gained in a relationship between two users:

- the storage capacity allocated for data packets from the user increases
- the probability of the each user choosing the other user as a trade partner
  increases

The users of the _eignewallet_ can mark users they know to be fairly
trustworthy. These can be users that are well known in the community or someone
the user knows personally. If a user does an successful _atomic swap_ with
another user, it will trust that user more.

The _eigenweb_ works on an _best effort_ basis. Malicious peers cannot do any
harm other than utilizing their full storage capacity (not more than a few
kilobytes).

Data packets are fully end-to-end encrypted. All connections in the _eigenweb_
operate _within_ the Tor network. Connections never reach an _exit node_. Each
user gets theirs own _hidden service_. Inbound and outbound connections are
therefore fully anonymous.

## References

[1]: Kamvar, Sepandar D., Mario T. Schlosser, and Hector Garcia-Molina. "The
EigenTrust Algorithm for Reputation Management in P2P Networks." _Proceedings of
the 11th International Conference on World Wide Web_, 2003.
https://nlp.stanford.edu/pubs/eigentrust.pdf.
