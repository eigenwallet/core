## Server

The server is simple. It receives PinRequests and PullRequests and responds to them.

### PinRequests

When it receives a PinRequest, a (PinRequest, InboundRequestId, ResponseChannel) tuple it is added to a queue.

In the event loop on every tick we check if there are any PinRequests in the queue.

If there are, we remove it from the queue and call an appropiate async storage function. We then insert a future into a FuturesUnordered. It yields a (Result<PinResponse, PinRejectReason>, InboundRequestId, ResponseChannel) tuple.

In the event loop we check if there is a future that is ready. If there is, we remove it from the FuturesUnordered and send the response to the client through the yielded ResponseChannel.

### PullRequests

When it receives a PullRequest, a (PullRequest, InboundRequestId, ResponseChannel) tuple it is added to a queue.

In the event loop on every tick we check if there are any PullRequests in the queue.

If there are, we remove it from the queue and call an appropiate async storage function. We then insert a future into a FuturesUnordered. It yields a (Result<PullResponse, PullRejectReason>, InboundRequestId, ResponseChannel) tuple.

In the event loop we check if there is a future that is ready. If there is, we remove it from the FuturesUnordered and send the response to the client through the yielded ResponseChannel.

## Client

The client is a little more complex.

It has a list of local message that it wants to be broadcasted through the network. It needs to make sure this list is kept up to date with the underlying storage layer. It must also prune messages that have an expired `msg.ttl`.

We only store the hash of the message in the client itself. If we need the message, we can look it up in the storage layer.

One client can connect to multiple servers.

We continuously check which messages the servers have each. If one of them is missing one of our messages, we send a PinRequest to the server to pin the message.

If we cannot dial a server, we backoff (for that server) and try again agressively in the beginning and more slowly later.

If a PinRequest fails due to a network error, we backoff (for that server) and try again agressively in the beginning and more slowly later.

If a server has rejected our PinRequest, we backoff (for that message) and try again much later.
