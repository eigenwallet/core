/**
 * Pages for the partial refund path of the swap.
 *
 * This path is taken when Alice only signs the partial refund transaction
 * (not the full refund). The flow is:
 *
 * 1. BtcPartialRefundPublished - TxPartialRefund is published
 * 2. BtcPartiallyRefunded - TxPartialRefund is confirmed
 * 3. Either:
 *    a. BtcAmnestyPublished -> BtcAmnestyReceived (Bob claims amnesty via TxRefundAmnesty)
 *    b. BtcRefundBurnPublished -> BtcRefundBurnt (Alice burns amnesty via TxRefundBurn)
 *       -> optionally BtcFinalAmnestyPublished -> BtcFinalAmnestyConfirmed (Alice grants final amnesty)
 */

export function BitcoinPartialRefundPublished() {
  return <>TxPartialRefund published</>;
}

export function BitcoinPartiallyRefunded() {
  return <>Bitcoin partially refunded</>;
}

export function BitcoinAmnestyPublished() {
  return <>TxAmnesty published</>;
}

export function BitcoinAmnestyReceived() {
  return <>Bitcoin amnesty received</>;
}

export function BitcoinRefundBurnPublished() {
  return <>TxRefundBurn published - Alice burned the amnesty output</>;
}

export function BitcoinRefundBurnt() {
  return <>Bitcoin refund is burnt - waiting for Alice to grant final amnesty</>;
}

export function BitcoinFinalAmnestyPublished() {
  return <>TxFinalAmnesty published - Alice granted final amnesty</>;
}

export function BitcoinFinalAmnestyConfirmed() {
  return <>Bitcoin final amnesty received - swap complete</>;
}
