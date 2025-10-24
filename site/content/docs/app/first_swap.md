## Completing your first atomic swap

### Prerequisites

 1. Available Bitcoin funds either already in your eigenwallet or in a separate Bitcoin wallet
 2. At least 30 minutes of time for the swap to run
 3. If necessary: the ability to run the app at some point during the refund period (12 hours until 36 hours after the start of the swap)

The protocol currently only supports swapping Bitcoin for Monero ([Why only in one direction?](/faq.html#why-only-in-one-direction)).

The minimum swap duration is 30 minutes.
This is necessary because the security of the protocol requires waiting for one Bitcoin transaction and one Monero transaction to be finalized as part of the swap. 
Due to unrealiable network conditions and other factors, the actual swap duration can be longer.

The protocol also requires that you have a _limited refund period_. 
It begins 12 hours after the start of the swap and lasts for 24 hours. 
Should your swap not complete successfully, it is _essential_ that you run the app at some point in this time frame.

Else, the maker will be able to forcibly take your Bitcoin.
This is necessary to guard makers against malicious takers who don't refund their Bitcoin (makers can only refund their Monero once the taker has refunded).

### 1. Selecting an offer

The first step to starting a trustless atomic swap is to navigate to the "Swap" tab.
After clicking "Click to view offers", the app will automatically gather offers from all available makers.
This may take a few seconds.


##### Topping up your Bitcoin balance

Of course, since we want to swap Bitcoin for Monero, we need to have some Bitcoin funds available.
You can see the balance of your internal Bitcoin wallet on the top of the screen. 
If you didn't already, this is the chance to transfer some Bitcoin to your internal wallet.
Simply copy the address by clicking on it or the "Copy" button and send some Bitcoin to it from another wallet/exchange.
You can also display the address as a QR code by clicking on the QR code icon.

##### Selecting the _right_ offer

Each maker has different exchange rates and minimum/maximum swap amounts.
For each maker you can see a few pieces of information:

 1. The maker's public identity and network address. 
 2. The exchange rate (how much Bitcoin they demand for 1 Monero)
 3. The _markup_ compared to centralized markets. This is how many percent the maker charges on top of the market rate. _(if you don't see this, enable "Query fiat prices" in the "Settings" tab)_
 4. The minimum and maximum swap amounts. You'll be able to swap any amount between them.

_Pro tip: you can hover over the amounts to see their value in fiat currency._

Generally, you want to select the offer with the lowest markup available for your desired swap amount.

![A screenshot of the app during the offer discovery phase](/imgs/screenshots/1-getting-offers.png)

Once you found an offer you're happy with and that's available for your Bitcoin amount, click on the "Select" button next to it.
