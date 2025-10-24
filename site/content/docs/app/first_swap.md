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
You _must_ be able to run the app at some point during this time frame.
Otherwise you ma _not_ be able to refund if the swap doesn't complete.

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

### 2. Confirming the swap

Once you have selected an offer, the app will connect to the maker and make final preparations for the swap.
You will see a confirmation page with the details on the swap:

 - The exact amount of Bitcoin you'll be sending and Monero you'll be receiving
 - The Bitcoin network fee required to initiate the swap
 - The exact exchange rate you'll be getting. _This might be slightly different from the one you selected due to network latency, caching, and other factors._

If you are happy with the details, click on the "Confirm" button to proceed.
Otherwise, click "Cancel" to go back to the previous page and select a different offer.

TODO: Screenshot of the confirmation page

### 3. Lean back an watch your swap complete

Once initiated, the app will automatically start executing the swap. 
You can see a progress bar at the bottom indicating at which step the swap currenty is.

TODO: Screenshot of the progress bar

The swap will go through four stages:

 1. Locking your Bitcoin
 2. Maker locks the Monero
 3. Maker redeems the Bitcoin
 4. You redeem the Monero

Steps 2 and 3 require network communication with the maker. 
If the swap is stuck at these steps, try restarting the app - this sometimes resolves networking issues.

If the swap doesn't finish within 6 hours, it will have to be refunded.

Once step 3 is complete, the swap is completed from a cryptographic standpoint.
The app retrieved the key necessary to access the locked Monero funds.
If the swap is stuck at this step, try restarting the app.

#### 3.1 Refunding if necessary

TODO: Screenshot of the refund timeline

If the swap doesn't progress to step 4 within 6 hours, it has to be refunded.
The app will start to show a timeline for this once the swap duration reaches 60 minutes.

The _refund period_ is the time frame during which you can refund your Bitcoin if the swap didn't complete.
It begins 12 hours after the start of the swap and lasts for 24 hours.
All you have to do is to *_make sure the app is running_* at some point during this time frame, but it is _essential_ that you do.
It will then automatically perform the necessary actions to refund your Bitcoin.

Should you miss the refund period, the maker will be able to forcibly take your Bitcoin.
_This means you may loose your funds!_
This is a necessary measure to protect makers from malicious actors.


### 4. Enjoy your Monero!

You just completed your first trustless atomic swap - congratulations!
You can manage the Monero funds in the "Wallet" tab with the Monero icon, or spend it as you wish.
It's your's after all.
