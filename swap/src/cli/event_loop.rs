use crate::cli::behaviour::{Behaviour, OutEvent};
use crate::monero;
use crate::network::cooperative_xmr_redeem_after_punish::{self, Request, Response};
use crate::network::encrypted_signature;
use crate::network::quote::BidQuote;
use crate::network::swap_setup::bob::NewSwap;
use crate::protocol::bob::swap::has_already_processed_transfer_proof;
use crate::protocol::bob::{BobState, State2};
use crate::protocol::Database;
use anyhow::{anyhow, bail, Context, Result};
use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};
use libp2p::request_response::{OutboundFailure, OutboundRequestId, ResponseChannel};
use libp2p::swarm::SwarmEvent;
use libp2p::{PeerId, Swarm};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use swap_core::bitcoin::EncryptedSignature;
use swap_p2p::protocols::redial;
use tracing::Instrument;
use uuid::Uuid;

// Timeout for the execution setup protocol within the event loop.
// If the behaviour does not respond within this time, we will consider the request failed.
// Also used to give up on retries within the EventLoopHandle.
static EXECUTION_SETUP_MAX_ELAPSED_TIME: Duration = Duration::from_secs(120);

// Used for deciding how long to retry request-response protocol requests where we want to give up eventually.
//
// This is used for:
// - Requesting quotes
// - Requesting cooperative XMR redeem
static REQUEST_RESPONSE_PROTOCOL_RETRY_MAX_ELASPED_TIME: Duration = Duration::from_secs(60);

// Used for deciding how long to wait at most between retries.
static RETRY_MAX_INTERVAL: Duration = Duration::from_secs(5);

#[allow(missing_debug_implementations)]
pub struct EventLoop {
    swarm: libp2p::Swarm<Behaviour>,
    db: Arc<dyn Database + Send + Sync>,

    // When a new `SwapEventLoopHandle` is created:
    // 1. a channel is created for the EventLoop to send transfer_proofs to SwapEventLoopHandle
    // 2. the corresponding PeerId of Alice is stored
    //
    // The sender of the channel is sent into this queue. The receiver is stored in the `SwapEventLoopHandle`.
    //
    // This is polled and then moved into `registered_swap_handlers`
    queued_swap_handlers: bmrng::unbounded::UnboundedRequestReceiverStream<
        (
            Uuid,
            PeerId,
            bmrng::unbounded::UnboundedRequestSender<monero::TransferProof, ()>,
            tracing::Span,
        ),
        (),
    >,
    registered_swap_handlers: HashMap<
        Uuid,
        (
            PeerId,
            bmrng::unbounded::UnboundedRequestSender<monero::TransferProof, ()>,
            tracing::Span,
        ),
    >,

    // These streams represents outgoing requests that we have to make (queues)
    //
    // Requests are keyed by the PeerId because they do not correspond to an existing swap yet
    quote_requests: bmrng::unbounded::UnboundedRequestReceiverStream<
        (PeerId, tracing::Span),
        Result<BidQuote, OutboundFailure>,
    >,
    // TODO: technically NewSwap.swap_id already contains the id of the swap
    execution_setup_requests: bmrng::unbounded::UnboundedRequestReceiverStream<
        (PeerId, NewSwap, tracing::Span),
        Result<State2>,
    >,

    // These streams represents outgoing requests that we have to make (queues)
    //
    // Requests are keyed by the swap_id because they correspond to a specific swap
    cooperative_xmr_redeem_requests: bmrng::unbounded::UnboundedRequestReceiverStream<
        (PeerId, Uuid, tracing::Span),
        Result<cooperative_xmr_redeem_after_punish::Response, OutboundFailure>,
    >,
    encrypted_signatures_requests: bmrng::unbounded::UnboundedRequestReceiverStream<
        (PeerId, Uuid, EncryptedSignature, tracing::Span),
        Result<(), OutboundFailure>,
    >,

    // These represents requests that are currently in-flight.
    // Meaning that we have sent them to Alice, but we have not yet received a response.
    // Once we get a response to a matching [`RequestId`], we will use the responder to relay the
    // response.
    inflight_quote_requests: HashMap<
        OutboundRequestId,
        (
            bmrng::unbounded::UnboundedResponder<Result<BidQuote, OutboundFailure>>,
            tracing::Span,
        ),
    >,
    inflight_encrypted_signature_requests: HashMap<
        OutboundRequestId,
        (
            bmrng::unbounded::UnboundedResponder<Result<(), OutboundFailure>>,
            tracing::Span,
        ),
    >,
    inflight_swap_setup: HashMap<
        (PeerId, Uuid),
        (
            bmrng::unbounded::UnboundedResponder<Result<State2>>,
            tracing::Span,
        ),
    >,
    inflight_cooperative_xmr_redeem_requests: HashMap<
        OutboundRequestId,
        (
            bmrng::unbounded::UnboundedResponder<
                Result<cooperative_xmr_redeem_after_punish::Response, OutboundFailure>,
            >,
            tracing::Span,
        ),
    >,

    /// The future representing the successful handling of an incoming transfer proof (by the state machine)
    ///
    /// Once we've sent a transfer proof to the ongoing swap, a future is inserted into this set
    /// which will resolve once the state machine has "processed" the transfer proof.
    ///
    /// The future will yield the swap_id and the response channel which are used to send an acknowledgement to Alice.
    pending_transfer_proof_acks: FuturesUnordered<BoxFuture<'static, (Uuid, ResponseChannel<()>)>>,

    /// Queue for adding peer addresses to the swarm
    add_peer_address_requests:
        bmrng::unbounded::UnboundedRequestReceiverStream<(PeerId, libp2p::Multiaddr), ()>,

    cached_quotes_sender: tokio::sync::watch::Sender<
        Vec<(PeerId, libp2p::Multiaddr, BidQuote, Option<semver::Version>)>,
    >,
}

impl EventLoop {
    pub fn new(
        swarm: Swarm<Behaviour>,
        db: Arc<dyn Database + Send + Sync>,
    ) -> Result<(Self, EventLoopHandle)> {
        // We still use a timeout here because we trust our own implementation of the swap setup protocol less than the libp2p library
        let (execution_setup_sender, execution_setup_receiver) =
            bmrng::unbounded::channel_with_timeout(EXECUTION_SETUP_MAX_ELAPSED_TIME);

        // It is okay to not have a timeout here, as timeouts are enforced by the request-response protocol
        let (encrypted_signature_sender, encrypted_signature_receiver) =
            bmrng::unbounded::channel();
        let (quote_sender, quote_receiver) = bmrng::unbounded::channel();
        let (cooperative_xmr_redeem_sender, cooperative_xmr_redeem_receiver) =
            bmrng::unbounded::channel();
        let (queued_transfer_proof_sender, queued_transfer_proof_receiver) =
            bmrng::unbounded::channel();
        let (add_peer_address_sender, add_peer_address_receiver) = bmrng::unbounded::channel();

        // TODO: We should probably differentiate between empty and none
        let (cached_quotes_sender, cached_quotes_receiver) =
            tokio::sync::watch::channel(Vec::new());

        let event_loop = EventLoop {
            swarm,
            db,
            queued_swap_handlers: queued_transfer_proof_receiver.into(),
            registered_swap_handlers: HashMap::default(),
            execution_setup_requests: execution_setup_receiver.into(),
            encrypted_signatures_requests: encrypted_signature_receiver.into(),
            cooperative_xmr_redeem_requests: cooperative_xmr_redeem_receiver.into(),
            quote_requests: quote_receiver.into(),
            inflight_quote_requests: HashMap::default(),
            inflight_swap_setup: HashMap::default(),
            inflight_encrypted_signature_requests: HashMap::default(),
            inflight_cooperative_xmr_redeem_requests: HashMap::default(),
            pending_transfer_proof_acks: FuturesUnordered::new(),
            add_peer_address_requests: add_peer_address_receiver.into(),
            cached_quotes_sender,
        };

        let handle = EventLoopHandle {
            execution_setup_sender,
            encrypted_signature_sender,
            cooperative_xmr_redeem_sender,
            quote_sender,
            queued_transfer_proof_sender,
            add_peer_address_sender,
            cached_quotes_receiver,
        };

        Ok((event_loop, handle))
    }

    pub async fn run(mut self) {
        loop {
            // Note: We are making very elaborate use of `select!` macro's feature here. Make sure to read the documentation thoroughly: https://docs.rs/tokio/1.4.0/tokio/macro.select.html
            tokio::select! {
                swarm_event = self.swarm.select_next_some() => {
                    match swarm_event {
                        SwarmEvent::Behaviour(OutEvent::QuoteReceived { id, response }) => {
                            if let Some((responder, span)) = self.inflight_quote_requests.remove(&id) {
                                let _span_guard = span.enter();

                                tracing::trace!(
                                    %id,
                                    "Received quote"
                                );

                                let _ = responder.respond(Ok(response));
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::SwapSetupCompleted { peer, swap_id, result }) => {
                            if let Some((responder, span)) = self.inflight_swap_setup.remove(&(peer, swap_id)) {
                                let _span_guard = span.enter();

                                tracing::trace!(
                                    %peer,
                                    "Processing swap setup completion"
                                );

                                let _ = responder.respond(*result);
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::TransferProofReceived { msg, channel, peer }) => {
                            tracing::trace!(
                                %peer,
                                %msg.swap_id,
                                "Received transfer proof"
                            );

                            let swap_id = msg.swap_id;

                            // Check if we have a registered handler for this swap
                            if let Some((expected_peer_id, sender, _)) = self.registered_swap_handlers.get(&swap_id) {
                                // Ensure the transfer proof is coming from the expected peer
                                if peer != *expected_peer_id {
                                    tracing::warn!(
                                        %swap_id,
                                        "Ignoring malicious transfer proof from {}, expected to receive it from {}",
                                        peer,
                                        expected_peer_id);
                                    continue;
                                }

                                // Send the transfer proof to the registered handler
                                match sender.send(msg.tx_lock_proof) {
                                    Ok(mut responder) => {
                                        // Insert a future that will resolve when the handle "takes the transfer proof out"
                                        self.pending_transfer_proof_acks.push(async move {
                                            let _ = responder.recv().await;
                                            (swap_id, channel)
                                        }.boxed());
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            %swap_id,
                                            %peer,
                                            error = ?e,
                                            "Failed to pass transfer proof to registered handler"
                                        );
                                    }
                                }

                                continue;
                            }

                            // Immediately acknowledge if we've already processed this transfer proof
                            // This handles the case where Alice didn't receive our previous acknowledgment
                            // and is retrying sending the transfer proof
                            match should_acknowledge_transfer_proof(self.db.clone(), swap_id, peer).await {
                                Ok(true) => {
                                    // We set this to a future that will resolve immediately, and returns the channel
                                    // This will be resolved in the next iteration of the event loop, and a response will be sent to Alice
                                    self.pending_transfer_proof_acks.push(async move {
                                        (swap_id, channel)
                                    }.boxed());

                                    // Skip evaluation of whether we should buffer the transfer proof
                                    // if we already acknowledged the transfer proof
                                    continue;
                                }
                                // TODO: Maybe we should log here?
                                Ok(false) => {}
                                Err(error) => {
                                    tracing::warn!(
                                        %swap_id,
                                        %peer,
                                        error = ?error,
                                        "Failed to evaluate if we should acknowledge the transfer proof, we will not respond at all"
                                    );
                                }
                            }

                            // Check if we should buffer the transfer proof
                            if let Err(error) = buffer_transfer_proof_if_needed(self.db.clone(), swap_id, peer, msg.tx_lock_proof).await {
                                tracing::warn!(
                                    %swap_id,
                                    %peer,
                                    error = ?error,
                                    "Failed to buffer transfer proof"
                                );
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::EncryptedSignatureAcknowledged { id }) => {
                            if let Some((responder, span)) = self.inflight_encrypted_signature_requests.remove(&id) {
                                let _span_guard = span.enter();
                                let _ = responder.respond(Ok(()));
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::CooperativeXmrRedeemFulfilled { id, swap_id, s_a, lock_transfer_proof }) => {
                            if let Some((responder, span)) = self.inflight_cooperative_xmr_redeem_requests.remove(&id) {
                                let _span_guard = span.enter();
                                let _ = responder.respond(Ok(Response::Fullfilled { s_a, swap_id, lock_transfer_proof }));
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::CooperativeXmrRedeemRejected { id, swap_id, reason }) => {
                            if let Some((responder, span)) = self.inflight_cooperative_xmr_redeem_requests.remove(&id) {
                                let _span_guard = span.enter();
                                let _ = responder.respond(Ok(Response::Rejected { reason, swap_id }));
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::Failure { peer, error }) => {
                            let span = self.get_peer_span(peer);
                            let _span_guard = span.enter();
                            tracing::warn!(%peer, err = ?error, "Communication error");
                            return;
                        }
                        SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                            let span = self.get_peer_span(peer_id);
                            let _span_guard = span.enter();
                            tracing::info!(%peer_id, peer_addr = %endpoint.get_remote_address(), "Connected to peer");
                        }
                        SwarmEvent::Dialing { peer_id: Some(peer_id), connection_id } => {
                            let span = self.get_peer_span(peer_id);
                            let _span_guard = span.enter();
                            tracing::debug!(%peer_id, %connection_id, "Dialing peer");
                        }
                        SwarmEvent::ConnectionClosed { peer_id, num_established, cause: Some(error), connection_id, endpoint } if num_established == 0 => {
                            let span = self.get_peer_span(peer_id);
                            let _span_guard = span.enter();
                            tracing::warn!(%peer_id, peer_addr = %endpoint.get_remote_address(), cause = ?error, %connection_id, "Lost connection to peer");
                        }
                        SwarmEvent::ConnectionClosed { peer_id, num_established, cause: None, .. } if num_established == 0 => {
                            // no error means the disconnection was requested
                            let span = self.get_peer_span(peer_id);
                            let _span_guard = span.enter();
                            tracing::info!(%peer_id, "Successfully closed connection to peer");
                        }
                        SwarmEvent::OutgoingConnectionError { peer_id: Some(peer_id),  error, connection_id } => {
                            let span = self.get_peer_span(peer_id);
                            let _span_guard = span.enter();
                            tracing::warn!(%peer_id, %connection_id, ?error, "Outgoing connection error to peer");
                        }
                        SwarmEvent::Behaviour(OutEvent::OutboundRequestResponseFailure {peer, error, request_id, protocol}) => {
                            tracing::error!(
                                %peer,
                                %request_id,
                                ?error,
                                %protocol,
                                "Failed to send request-response request to peer");

                            // If we fail to send a request-response request, we should notify the responder that the request failed
                            // We will remove the responder from the inflight requests and respond with an error

                            // Check for encrypted signature requests
                            if let Some((responder, span)) = self.inflight_encrypted_signature_requests.remove(&request_id) {
                                let _span_guard = span.enter();
                                let _ = responder.respond(Err(error));
                                continue;
                            }

                            // Check for quote requests
                            if let Some((responder, span)) = self.inflight_quote_requests.remove(&request_id) {
                                let _span_guard = span.enter();
                                let _ = responder.respond(Err(error));
                                continue;
                            }

                            // Check for cooperative xmr redeem requests
                            if let Some((responder, span)) = self.inflight_cooperative_xmr_redeem_requests.remove(&request_id) {
                                let _span_guard = span.enter();
                                let _ = responder.respond(Err(error));
                                continue;
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::InboundRequestResponseFailure {peer, error, request_id, protocol}) => {
                            tracing::error!(
                                %peer,
                                %request_id,
                                ?error,
                                %protocol,
                                "Failed to receive or send response for request-response request from peer");
                        }
                        SwarmEvent::Behaviour(OutEvent::Redial(redial::Event::ScheduledRedial { peer, next_dial_in })) => {
                            tracing::trace!(
                                %peer,
                                seconds_until_next_redial = %next_dial_in.as_secs(),
                                "Scheduled redial for peer"
                            );
                        }
                        SwarmEvent::Behaviour(OutEvent::CachedQuotes { quotes }) => {
                            tracing::trace!(
                                ?quotes,
                                "Received cached quotes"
                            );

                            let _ = self.cached_quotes_sender.send(quotes);
                        }
                        _ => {}
                    }
                },

                // Handle to-be-sent outgoing requests for all our network protocols.
                Some(((peer_id, span), responder)) = self.quote_requests.next().fuse() => {
                    let _span_guard = span.enter();

                    let outbound_request_id = self.swarm.behaviour_mut().direct_quote.send_request(&peer_id, ());
                    self.inflight_quote_requests.insert(outbound_request_id, (responder, span.clone()));

                    tracing::trace!(
                        %peer_id,
                        %outbound_request_id,
                        "Dispatching outgoing quote request"
                    );
                },
                Some(((peer_id, swap_id, tx_redeem_encsig, span), responder)) = self.encrypted_signatures_requests.next().fuse() => {
                    let _span_guard = span.enter();

                    let request = encrypted_signature::Request {
                        swap_id,
                        tx_redeem_encsig
                    };

                    let outbound_request_id = self.swarm.behaviour_mut().encrypted_signature.send_request(&peer_id, request);
                    self.inflight_encrypted_signature_requests.insert(outbound_request_id, (responder, span.clone()));

                    tracing::trace!(
                        %peer_id,
                        %swap_id,
                        %outbound_request_id,
                        "Dispatching outgoing encrypted signature"
                    );
                },
                Some(((peer_id, swap_id, span), responder)) = self.cooperative_xmr_redeem_requests.next().fuse() => {
                    let _span_guard = span.enter();

                    let outbound_request_id = self.swarm.behaviour_mut().cooperative_xmr_redeem.send_request(&peer_id, Request {
                        swap_id
                    });
                    self.inflight_cooperative_xmr_redeem_requests.insert(outbound_request_id, (responder, span.clone()));

                    tracing::trace!(
                        %peer_id,
                        %swap_id,
                        %outbound_request_id,
                        "Dispatching outgoing cooperative xmr redeem request"
                    );
                },

                // Instruct the swap setup behaviour to do a swap setup request
                // The behaviour will instruct the swarm to dial Alice, so we don't need to check if we are connected
                Some(((alice_peer_id, swap, span), responder)) = self.execution_setup_requests.next().fuse() => {
                    let swap_id = swap.swap_id.clone();

                    // We await the start of the swap setup
                    // We use an async block to instrument the future, but we don't use `move`
                    // so `self` is borrowed, not moved.
                    async {
                        self.swarm.behaviour_mut().swap_setup.queue_new_swap(alice_peer_id, swap).await;
                    }
                    .instrument(span.clone())
                    .await;

                    self.inflight_swap_setup.insert((alice_peer_id, swap_id), (responder, span.clone()));

                    let _guard = span.enter();
                    tracing::trace!(
                        %alice_peer_id,
                        "Dispatching outgoing execution setup request"
                    );
                },
                // Send an acknowledgement to Alice once the EventLoopHandle has processed a received transfer proof
                Some((swap_id, response_channel)) = self.pending_transfer_proof_acks.next() => {
                    tracing::trace!(
                        %swap_id,
                        "Dispatching outgoing transfer proof acknowledgment");

                    // We do not check if we are connected to Alice here because responding on a channel
                    // which has been dropped works even if a new connections has been established since
                    // will not work because because a channel is always bounded to one connection
                    if self.swarm.behaviour_mut().transfer_proof.send_response(response_channel, ()).is_err() {
                        tracing::warn!("Failed to send acknowledgment to Alice that we have received the transfer proof");
                    } else {
                        tracing::info!("Sent acknowledgment to Alice that we have received the transfer proof");
                    }
                },

                Some(((swap_id, peer_id, sender, span), responder)) = self.queued_swap_handlers.next().fuse() => {
                    let _guard = span.enter();
                    tracing::trace!(%swap_id, %peer_id, "Registering swap handle for a swap internally inside the event loop");

                    // This registers the swap_id -> peer_id and swap_id -> transfer_proof_sender
                    self.registered_swap_handlers.insert(swap_id, (peer_id, sender, span.clone()));

                    // Instruct the swarm to contineously redial the peer
                    // TODO: We must remove it again once the swap is complete, otherwise we will redial indefinitely
                    self.swarm.behaviour_mut().redial.add_peer(peer_id);

                    // Acknowledge the registration
                    let _ = responder.respond(());
                },

                Some(((peer_id, addr), responder)) = self.add_peer_address_requests.next().fuse() => {
                    tracing::trace!(%peer_id, %addr, "Adding peer address to swarm");
                    self.swarm.add_peer_address(peer_id, addr);
                    let _ = responder.respond(());
                },
            }
        }
    }

    fn get_peer_span(&self, peer_id: PeerId) -> tracing::Span {
        let span = tracing::debug_span!("peer_context", %peer_id);

        for (peer, _, s) in self.registered_swap_handlers.values() {
            if *peer == peer_id {
                span.follows_from(s);
            }
        }
        span
    }
}

#[derive(Debug, Clone)]
pub struct EventLoopHandle {
    /// When a (PeerId, NewSwap) tuple is sent into this channel, the EventLoop will:
    /// 1. Trigger the swap setup protocol with the specified peer to negotiate the swap parameters
    /// 2. Return the resulting State2 if successful
    /// 3. Return an anyhow error if the request fails
    execution_setup_sender:
        bmrng::unbounded::UnboundedRequestSender<(PeerId, NewSwap, tracing::Span), Result<State2>>,

    /// When a (PeerId, Uuid, EncryptedSignature) tuple is sent into this channel, the EventLoop will:
    /// 1. Send the encrypted signature to the specified peer over the network
    /// 2. Return Ok(()) if the peer acknowledges receipt, or
    /// 3. Return an OutboundFailure error if the request fails
    encrypted_signature_sender: bmrng::unbounded::UnboundedRequestSender<
        (PeerId, Uuid, EncryptedSignature, tracing::Span),
        Result<(), OutboundFailure>,
    >,

    /// When a PeerId is sent into this channel, the EventLoop will:
    /// 1. Request a price quote from the specified peer
    /// 2. Return the quote if successful
    /// 3. Return an OutboundFailure error if the request fails
    quote_sender: bmrng::unbounded::UnboundedRequestSender<
        (PeerId, tracing::Span),
        Result<BidQuote, OutboundFailure>,
    >,

    /// When a (PeerId, Uuid) tuple is sent into this channel, the EventLoop will:
    /// 1. Request the specified peer's cooperation in redeeming the Monero for the given swap
    /// 2. Return a response object (Fullfilled or Rejected), if the network request is successful
    ///    The Fullfilled object contains the keys required to redeem the Monero
    /// 3. Return an OutboundFailure error if the network request fails
    cooperative_xmr_redeem_sender: bmrng::unbounded::UnboundedRequestSender<
        (PeerId, Uuid, tracing::Span),
        Result<cooperative_xmr_redeem_after_punish::Response, OutboundFailure>,
    >,

    queued_transfer_proof_sender: bmrng::unbounded::UnboundedRequestSender<
        (
            Uuid,
            PeerId,
            bmrng::unbounded::UnboundedRequestSender<monero::TransferProof, ()>,
            tracing::Span,
        ),
        (),
    >,

    /// Channel for adding peer addresses to the swarm
    add_peer_address_sender:
        bmrng::unbounded::UnboundedRequestSender<(PeerId, libp2p::Multiaddr), ()>,

    // TODO: Extract the Vec<_> into its own struct (QuotesBatch?)
    cached_quotes_receiver: tokio::sync::watch::Receiver<
        Vec<(PeerId, libp2p::Multiaddr, BidQuote, Option<semver::Version>)>,
    >,
}

impl EventLoopHandle {
    pub fn cached_quotes(
        &self,
    ) -> tokio::sync::watch::Receiver<
        Vec<(PeerId, libp2p::Multiaddr, BidQuote, Option<semver::Version>)>,
    > {
        self.cached_quotes_receiver.clone()
    }

    /// Adds a peer address to the swarm
    pub async fn queue_peer_address(
        &mut self,
        peer_id: PeerId,
        addr: libp2p::Multiaddr,
    ) -> Result<()> {
        self.add_peer_address_sender
            .send((peer_id, addr))
            .context("Failed to queue peer address into event loop")?;

        Ok(())
    }

    /// Creates a SwapEventLoopHandle for a specific swap
    /// This registers the swap's transfer proof receiver with the event loop
    pub async fn swap_handle(
        &mut self,
        peer_id: PeerId,
        swap_id: Uuid,
    ) -> Result<SwapEventLoopHandle> {
        // Create a channel for sending transfer proofs from the `EventLoop` to the `SwapEventLoopHandle`
        //
        // The sender is stored in the `EventLoop`. The receiver is stored in the `SwapEventLoopHandle`.
        let (transfer_proof_sender, transfer_proof_receiver) = bmrng::unbounded_channel();
        let span = tracing::Span::current();

        // Register this sender in the `EventLoop`
        // It is put into the queue and then later moved into `registered_transfer_proof_senders`
        //
        // We use `send(...) instead of send_receive(...)` because the event loop needs to be running for this to respond
        self.queued_transfer_proof_sender
            .send((swap_id, peer_id, transfer_proof_sender, span))
            .context("Failed to queue transfer proof sender into event loop")?;

        Ok(SwapEventLoopHandle {
            handle: self.clone(),
            peer_id,
            swap_id,
            transfer_proof_receiver: Some(transfer_proof_receiver),
        })
    }

    /// Sets up a swap with the specified peer
    ///
    /// This will retry until the maximum elapsed time is reached. It is therefore fallible.
    pub async fn setup_swap(&mut self, peer_id: PeerId, swap: NewSwap) -> Result<State2> {
        let span = tracing::Span::current();
        tracing::debug!(swap = ?swap, %peer_id, "Sending swap setup request");

        let backoff =
            retry::give_up_eventually(RETRY_MAX_INTERVAL, EXECUTION_SETUP_MAX_ELAPSED_TIME);

        backoff::future::retry_notify(backoff, || async {
            match self.execution_setup_sender.send_receive((peer_id, swap.clone(), span.clone())).await {
                Ok(Ok(state2)) => {
                    Ok(state2)
                }
                // These are errors thrown by the swap_setup/bob behaviour
                Ok(Err(err)) => {
                    Err(backoff::Error::transient(err.context("A network error occurred while setting up the swap")))
                }
                // This will happen if we don't establish a connection to Alice within the timeout of the MPSC channel
                // The protocol does not dial Alice it self
                // This is handled by redial behaviour
                Err(bmrng::error::RequestError::RecvTimeoutError) => {
                    Err(backoff::Error::permanent(anyhow!("We failed to setup the swap in the allotted time by the event loop channel")))
                }
                Err(_) => {
                    unreachable!("We never drop the receiver of the execution setup channel, so this should never happen")
                }
            }
        }, |err, wait_time: Duration| {
            tracing::warn!(
                error = ?err,
                "Failed to setup swap. We will retry in {} seconds",
                wait_time.as_secs()
            );
        })
        .await
        .context("Failed to setup swap after retries")
    }

    /// Requests a quote from the specified peer
    ///
    /// This will retry until the maximum elapsed time is reached. It is therefore fallible.
    pub async fn request_quote(&mut self, peer_id: PeerId) -> Result<BidQuote> {
        tracing::debug!(%peer_id, "Requesting quote");

        // We want to give up eventually here
        let backoff = retry::give_up_eventually(
            RETRY_MAX_INTERVAL,
            REQUEST_RESPONSE_PROTOCOL_RETRY_MAX_ELASPED_TIME,
        );

        let span = tracing::Span::current();
        backoff::future::retry_notify(backoff, || async {
            match self.quote_sender.send_receive((peer_id, span.clone())).await {
                Ok(Ok(quote)) => Ok(quote),
                Ok(Err(err)) => {
                    Err(backoff::Error::transient(anyhow!(err).context("A network error occurred while requesting a quote")))
                }
                Err(_) => {
                    unreachable!("We initiate the quote channel without a timeout and store both the sender and receiver in the same struct, so this should never happen");
                }
            }
        }, |err, wait_time: Duration| {
            tracing::warn!(
                error = ?err,
                "Failed to request quote. We will retry in {} seconds",
                wait_time.as_secs()
            )
        })
        .await
        .context("Failed to request quote after retries")
    }

    /// Requests the cooperative XMR redeem from the specified peer
    ///
    /// This will retry until the maximum elapsed time is reached. It is therefore fallible.
    pub async fn request_cooperative_xmr_redeem(
        &mut self,
        peer_id: PeerId,
        swap_id: Uuid,
    ) -> Result<Response> {
        let span = tracing::Span::current();
        tracing::debug!(%peer_id, %swap_id, "Requesting cooperative XMR redeem");

        // We want to give up eventually here
        let backoff = retry::give_up_eventually(
            RETRY_MAX_INTERVAL,
            REQUEST_RESPONSE_PROTOCOL_RETRY_MAX_ELASPED_TIME,
        );

        backoff::future::retry_notify(backoff, || async {
            match self.cooperative_xmr_redeem_sender.send_receive((peer_id, swap_id, span.clone())).await {
                Ok(Ok(response)) => Ok(response),
                Ok(Err(err)) => {
                    Err(backoff::Error::transient(anyhow!(err).context("A network error occurred while requesting cooperative XMR redeem")))
                }
                Err(_) => {
                    unreachable!("We initiate the cooperative xmr redeem channel without a timeout and store both the sender and receiver in the same struct, so this should never happen");
                }
            }
        }, |err, wait_time: Duration| {
            tracing::warn!(
                error = ?err,
                "Failed to request cooperative XMR redeem. We will retry in {} seconds",
                wait_time.as_secs()
            )
        })
        .await
        .context("Failed to request cooperative XMR redeem after retries")
    }

    /// Sends an encrypted signature to the specified peer
    ///
    /// This will retry indefinitely until we succeed. It is therefore infalible.
    pub async fn send_encrypted_signature(
        &mut self,
        peer_id: PeerId,
        swap_id: Uuid,
        tx_redeem_encsig: EncryptedSignature,
    ) -> () {
        let span = tracing::Span::current();
        tracing::debug!(%peer_id, %swap_id, "Sending encrypted signature");

        // We will retry indefinitely until we succeed
        let backoff = retry::never_give_up(RETRY_MAX_INTERVAL);

        backoff::future::retry_notify(backoff, || async {
            match self.encrypted_signature_sender.send_receive((peer_id, swap_id, tx_redeem_encsig.clone(), span.clone())).await {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(err)) => {
                    Err(backoff::Error::transient(anyhow!(err).context("A network error occurred while sending the encrypted signature")))
                }
                Err(_) => {
                    unreachable!("We initiate the encrypted signature channel without a timeout and store both the sender and receiver in the same struct, so this should never happen");
                }
            }
        }, |err, wait_time: Duration| {
            tracing::warn!(
                error = ?err,
                "Failed to send encrypted signature. We will retry in {} seconds",
                wait_time.as_secs()
            )
        })
        .await
        .expect("we should never run out of retries when sending an encrypted signature")
    }
}

#[derive(Debug)]
pub struct SwapEventLoopHandle {
    handle: EventLoopHandle,
    peer_id: PeerId,
    swap_id: Uuid,
    transfer_proof_receiver:
        Option<bmrng::unbounded::UnboundedRequestReceiver<monero::TransferProof, ()>>,
}

impl SwapEventLoopHandle {
    pub async fn recv_transfer_proof(&mut self) -> Result<monero::TransferProof> {
        let receiver = self
            .transfer_proof_receiver
            .as_mut()
            .context("Transfer proof receiver not available")?;

        let (transfer_proof, responder) = receiver
            .recv()
            .await
            .context("Failed to receive transfer proof")?;

        responder
            .respond(())
            .context("Failed to acknowledge receipt of transfer proof")?;

        Ok(transfer_proof)
    }

    pub async fn send_encrypted_signature(&mut self, tx_redeem_encsig: EncryptedSignature) -> () {
        self.handle
            .send_encrypted_signature(self.peer_id, self.swap_id, tx_redeem_encsig)
            .await
    }

    pub async fn request_cooperative_xmr_redeem(&mut self) -> Result<Response> {
        self.handle
            .request_cooperative_xmr_redeem(self.peer_id, self.swap_id)
            .await
    }

    pub async fn setup_swap(&mut self, swap: NewSwap) -> Result<State2> {
        self.handle.setup_swap(self.peer_id, swap).await
    }

    pub async fn request_quote(&mut self) -> Result<BidQuote> {
        self.handle.request_quote(self.peer_id).await
    }
}

/// Returns Ok(true) if we should acknowledge the transfer proof
///
/// - Checks if the peer id is the expected peer id
/// - Checks if the state indicates that we have already processed the transfer proof
async fn should_acknowledge_transfer_proof(
    db: Arc<dyn Database + Send + Sync>,
    swap_id: Uuid,
    peer_id: PeerId,
) -> Result<bool> {
    let expected_peer_id = db.get_peer_id(swap_id).await.context(
        "Failed to get peer id for swap to check if we should acknowledge the transfer proof",
    )?;

    // If the peer id is not the expected peer id, we should not acknowledge the transfer proof
    // This is to prevent malicious requests
    if expected_peer_id != peer_id {
        bail!("Expected peer id {} but got {}", expected_peer_id, peer_id);
    }

    let state = db.get_state(swap_id).await.context(
        "Failed to get state for swap to check if we should acknowledge the transfer proof",
    )?;
    let state: BobState = state.try_into().context(
        "Failed to convert state to BobState to check if we should acknowledge the transfer proof",
    )?;

    Ok(has_already_processed_transfer_proof(&state))
}

/// Buffers the transfer proof in the database if its from the expected peer
async fn buffer_transfer_proof_if_needed(
    db: Arc<dyn Database + Send + Sync>,
    swap_id: Uuid,
    peer_id: PeerId,
    transfer_proof: monero::TransferProof,
) -> Result<()> {
    let expected_peer_id = db.get_peer_id(swap_id).await.context(
        "Failed to get peer id for swap to check if we should buffer the transfer proof",
    )?;

    if expected_peer_id != peer_id {
        bail!("Expected peer id {} but got {}", expected_peer_id, peer_id);
    }

    db.insert_buffered_transfer_proof(swap_id, transfer_proof)
        .await
        .context("Failed to buffer transfer proof in database")
}

mod retry {
    use std::time::Duration;

    // Constructs a retry config that will retry indefinitely
    pub(crate) fn never_give_up(max_interval: Duration) -> backoff::ExponentialBackoff {
        create_retry_config(max_interval, None)
    }

    // Constructs a retry config that will retry for a given amount of time
    pub(crate) fn give_up_eventually(
        max_interval: Duration,
        max_elapsed_time: Duration,
    ) -> backoff::ExponentialBackoff {
        create_retry_config(max_interval, max_elapsed_time)
    }

    fn create_retry_config(
        max_interval: Duration,
        max_elapsed_time: impl Into<Option<Duration>>,
    ) -> backoff::ExponentialBackoff {
        backoff::ExponentialBackoffBuilder::new()
            .with_max_interval(max_interval)
            .with_max_elapsed_time(max_elapsed_time.into())
            .build()
    }
}
