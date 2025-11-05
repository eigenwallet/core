use crate::cli::behaviour::{Behaviour, OutEvent};
use crate::monero;
use crate::network::cooperative_xmr_redeem_after_punish::{self, Request, Response};
use crate::network::encrypted_signature;
use crate::network::quote::BidQuote;
use crate::network::swap_setup::bob::NewSwap;
use crate::protocol::bob::swap::has_already_processed_transfer_proof;
use crate::protocol::bob::{BobState, State2};
use crate::protocol::Database;
use anyhow::{anyhow, Context, Result};
use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};
use libp2p::request_response::{OutboundFailure, OutboundRequestId, ResponseChannel};
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::SwarmEvent;
use libp2p::{PeerId, Swarm};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use swap_core::bitcoin::EncryptedSignature;
use swap_p2p::protocols::redial;
use uuid::Uuid;

static REQUEST_RESPONSE_PROTOCOL_TIMEOUT: Duration = Duration::from_secs(60);
static EXECUTION_SETUP_PROTOCOL_TIMEOUT: Duration = Duration::from_secs(120);

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
    queued_swap_handlers: bmrng::RequestReceiverStream<
        (
            Uuid,
            PeerId,
            bmrng::RequestSender<monero::TransferProof, ()>,
        ),
        (),
    >,
    registered_swap_handlers:
        HashMap<Uuid, (PeerId, bmrng::RequestSender<monero::TransferProof, ()>)>,

    // These streams represents outgoing requests that we have to make (queues)
    //
    // Requests are keyed by the PeerId because they do not correspond to any swap
    quote_requests: bmrng::RequestReceiverStream<PeerId, Result<BidQuote, OutboundFailure>>,
    execution_setup_requests: bmrng::RequestReceiverStream<(PeerId, NewSwap), Result<State2>>,

    // These streams represents outgoing requests that we have to make (queues)
    //
    // Requests are keyed by the swap_id because they correspond to a specific swap
    cooperative_xmr_redeem_requests: bmrng::RequestReceiverStream<
        (PeerId, Uuid),
        Result<cooperative_xmr_redeem_after_punish::Response, OutboundFailure>,
    >,
    encrypted_signatures_requests: bmrng::RequestReceiverStream<
        (PeerId, Uuid, EncryptedSignature),
        Result<(), OutboundFailure>,
    >,

    // These represents requests that are currently in-flight.
    // Meaning that we have sent them to Alice, but we have not yet received a response.
    // Once we get a response to a matching [`RequestId`], we will use the responder to relay the
    // response.
    inflight_quote_requests:
        HashMap<OutboundRequestId, bmrng::Responder<Result<BidQuote, OutboundFailure>>>,
    inflight_encrypted_signature_requests:
        HashMap<OutboundRequestId, bmrng::Responder<Result<(), OutboundFailure>>>,
    inflight_swap_setup: HashMap<(PeerId, Uuid), bmrng::Responder<Result<State2>>>,
    inflight_cooperative_xmr_redeem_requests: HashMap<
        OutboundRequestId,
        bmrng::Responder<Result<cooperative_xmr_redeem_after_punish::Response, OutboundFailure>>,
    >,

    /// The future representing the successful handling of an incoming transfer proof (by the state machine)
    ///
    /// Once we've sent a transfer proof to the ongoing swap, a future is inserted into this set
    /// which will resolve once the state machine has "processes" the transfer proof.
    ///
    /// The future will yield the swap_id and the response channel which are used to send an acknowledgement to Alice.
    pending_transfer_proof_acks: FuturesUnordered<BoxFuture<'static, (Uuid, ResponseChannel<()>)>>,
}

impl EventLoop {
    fn swap_peer_id(&self, swap_id: &Uuid) -> Option<PeerId> {
        self.registered_swap_handlers
            .get(swap_id)
            .map(|(peer_id, _)| *peer_id)
    }

    pub fn new(
        swarm: Swarm<Behaviour>,
        db: Arc<dyn Database + Send + Sync>,
    ) -> Result<(Self, EventLoopHandle)> {
        // We still use a timeout here, because this protocol does not dial Alice itself
        // and we want to fail if we cannot reach Alice
        let (execution_setup_sender, execution_setup_receiver) =
            bmrng::channel_with_timeout(1, EXECUTION_SETUP_PROTOCOL_TIMEOUT);

        // It is okay to not have a timeout here, as timeouts are enforced by the request-response protocol
        let (encrypted_signature_sender, encrypted_signature_receiver) = bmrng::channel(1);
        let (quote_sender, quote_receiver) = bmrng::channel(1);
        let (cooperative_xmr_redeem_sender, cooperative_xmr_redeem_receiver) = bmrng::channel(1);
        let (queued_transfer_proof_sender, queued_transfer_proof_receiver) = bmrng::channel(1);

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
        };

        let handle = EventLoopHandle {
            execution_setup_sender,
            encrypted_signature_sender,
            cooperative_xmr_redeem_sender,
            quote_sender,
            queued_transfer_proof_sender,
        };

        Ok((event_loop, handle))
    }

    pub async fn run(mut self) {
        tracing::info!("Bob's event loop started");

        loop {
            // Note: We are making very elaborate use of `select!` macro's feature here. Make sure to read the documentation thoroughly: https://docs.rs/tokio/1.4.0/tokio/macro.select.html
            tokio::select! {
                swarm_event = self.swarm.select_next_some() => {
                    match swarm_event {
                        SwarmEvent::Behaviour(OutEvent::QuoteReceived { id, response }) => {
                            tracing::trace!(
                                %id,
                                "Received quote"
                            );

                            if let Some(responder) = self.inflight_quote_requests.remove(&id) {
                                let _ = responder.respond(Ok(response));
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::SwapSetupCompleted { peer, swap_id, result }) => {
                            tracing::trace!(
                                %peer,
                                "Processing swap setup completion"
                            );

                            if let Some(responder) = self.inflight_swap_setup.remove(&(peer, swap_id)) {
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
                            if let Some((expected_peer_id, sender)) = self.registered_swap_handlers.get(&swap_id) {
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
                                match sender.send(msg.tx_lock_proof).await {
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
                            if let Ok(state) = self.db.get_state(swap_id).await {
                                // TODO: This could panic if the database contains an invalid state
                                // TODO: We should warn instead of panicking
                                let state: BobState = state.try_into()
                                    .expect("Bobs database only contains Bob states");

                                if has_already_processed_transfer_proof(&state) {
                                    tracing::warn!("Received transfer proof for swap {} but we are already in state {}. Acknowledging immediately. Alice most likely did not receive the acknowledgment when we sent it before", swap_id, state);

                                    // We set this to a future that will resolve immediately, and returns the channel
                                    // This will be resolved in the next iteration of the event loop, and a response will be sent to Alice
                                    self.pending_transfer_proof_acks.push(async move {
                                        (swap_id, channel)
                                    }.boxed());

                                    continue;
                                }
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::EncryptedSignatureAcknowledged { id }) => {
                            if let Some(responder) = self.inflight_encrypted_signature_requests.remove(&id) {
                                let _ = responder.respond(Ok(()));
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::CooperativeXmrRedeemFulfilled { id, swap_id, s_a, lock_transfer_proof }) => {
                            if let Some(responder) = self.inflight_cooperative_xmr_redeem_requests.remove(&id) {
                                let _ = responder.respond(Ok(Response::Fullfilled { s_a, swap_id, lock_transfer_proof }));
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::CooperativeXmrRedeemRejected { id, swap_id, reason }) => {
                            if let Some(responder) = self.inflight_cooperative_xmr_redeem_requests.remove(&id) {
                                let _ = responder.respond(Ok(Response::Rejected { reason, swap_id }));
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::Failure { peer, error }) => {
                            tracing::warn!(%peer, err = ?error, "Communication error");
                            return;
                        }
                        SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                            tracing::info!(peer_id = %endpoint.get_remote_address(), "Connected to peer");
                        }
                        SwarmEvent::Dialing { peer_id: Some(peer_id), connection_id } => {
                            tracing::debug!(%peer_id, %connection_id, "Dialing peer");
                        }
                        SwarmEvent::ConnectionClosed { peer_id, endpoint, num_established, cause: Some(error), connection_id } if num_established == 0 => {
                            tracing::warn!(peer_id = %endpoint.get_remote_address(), cause = ?error, %connection_id, "Lost connection to peer");
                        }
                        SwarmEvent::ConnectionClosed { peer_id, num_established, cause: None, .. } if num_established == 0 => {
                            // no error means the disconnection was requested
                            tracing::info!(%peer_id, "Successfully closed connection to peer");
                        }
                        SwarmEvent::OutgoingConnectionError { peer_id: Some(peer_id),  error, connection_id } => {
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
                            if let Some(responder) = self.inflight_encrypted_signature_requests.remove(&request_id) {
                                let _ = responder.respond(Err(error));
                                continue;
                            }

                            // Check for quote requests
                            if let Some(responder) = self.inflight_quote_requests.remove(&request_id) {
                                let _ = responder.respond(Err(error));
                                continue;
                            }

                            // Check for cooperative xmr redeem requests
                            if let Some(responder) = self.inflight_cooperative_xmr_redeem_requests.remove(&request_id) {
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
                        SwarmEvent::Behaviour(OutEvent::Redial(redial::Event::Redialing { peer })) => {
                            tracing::trace!(
                                %peer,
                                "Redialing peer"
                            );
                        }
                        _ => {}
                    }
                },

                // Handle to-be-sent outgoing requests for all our network protocols.
                Some((peer_id, responder)) = self.quote_requests.next().fuse() => {
                    tracing::trace!(
                        %peer_id,
                        "Dispatching outgoing quote request"
                    );

                    let id = self.swarm.behaviour_mut().quote.send_request(&peer_id, ());
                    self.inflight_quote_requests.insert(id, responder);
                },
                Some(((peer_id, swap_id, tx_redeem_encsig), responder)) = self.encrypted_signatures_requests.next().fuse() => {
                    let request = encrypted_signature::Request {
                        swap_id,
                        tx_redeem_encsig
                    };

                    let outbound_request_id = self.swarm.behaviour_mut().encrypted_signature.send_request(&peer_id, request);
                    self.inflight_encrypted_signature_requests.insert(outbound_request_id, responder);

                    tracing::trace!(
                        %peer_id,
                        %swap_id,
                        %outbound_request_id,
                        "Dispatching outgoing encrypted signature"
                    );
                },
                Some(((peer_id, swap_id), responder)) = self.cooperative_xmr_redeem_requests.next().fuse() => {
                    let outbound_request_id = self.swarm.behaviour_mut().cooperative_xmr_redeem.send_request(&peer_id, Request {
                        swap_id
                    });
                    self.inflight_cooperative_xmr_redeem_requests.insert(outbound_request_id, responder);

                    tracing::trace!(
                        %peer_id,
                        %swap_id,
                        %outbound_request_id,
                        "Dispatching outgoing cooperative xmr redeem request"
                    );
                },

                // We use `self.swarm.is_connected` as a guard to "buffer" requests until we are connected.
                // because the protocol does not dial Alice itself
                // (unlike request-response above)
                Some(((alice_peer_id, swap), responder)) = self.execution_setup_requests.next().fuse() => {
                    let swap_id = swap.swap_id.clone();

                    tracing::trace!(
                        %alice_peer_id,
                        "Dispatching outgoing execution setup request"
                    );

                    // TODO: handle the error here
                    let _ = self.swarm.dial(DialOpts::peer_id(alice_peer_id).condition(PeerCondition::Disconnected).build());
                    self.swarm.behaviour_mut().swap_setup.start(alice_peer_id, swap).await;

                    self.inflight_swap_setup.insert((alice_peer_id, swap_id), responder);
                },

                // Send an acknowledgement to Alice once the EventLoopHandle has processed a received transfer proof
                // We use `self.swarm.is_connected` as a guard to "buffer" requests until we are connected.
                //
                // Why do we do this here but not for the other request-response channels?
                // This is the only request, we don't have a retry mechanism for. We lazily send this.
                Some((swap_id, response_channel)) = self.pending_transfer_proof_acks.next() => {
                    if let Some(peer_id) = self.swap_peer_id(&swap_id) {
                        if self.swarm.is_connected(&peer_id) {
                            if self.swarm.behaviour_mut().transfer_proof.send_response(response_channel, ()).is_err() {
                                tracing::warn!("Failed to send acknowledgment to Alice that we have received the transfer proof");
                            } else {
                                tracing::info!("Sent acknowledgment to Alice that we have received the transfer proof");
                            }
                        }
                    }
                },

                Some(((swap_id, peer_id, sender), responder)) = self.queued_swap_handlers.next().fuse() => {
                    tracing::trace!(%swap_id, %peer_id, "Registering swap handle for a swap internally inside the event loop");

                    // This registers the swap_id -> peer_id and swap_id -> transfer_proof_sender
                    self.registered_swap_handlers.insert(swap_id, (peer_id, sender));

                    // Instruct the swarm to contineously redial the peer
                    self.swarm.behaviour_mut().redial.add_peer(peer_id);

                    // Acknowledge the registration
                    let _ = responder.respond(());
                },
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct EventLoopHandle {
    /// When a (PeerId, NewSwap) tuple is sent into this channel, the EventLoop will:
    /// 1. Trigger the swap setup protocol with the specified peer to negotiate the swap parameters
    /// 2. Return the resulting State2 if successful
    /// 3. Return an anyhow error if the request fails
    execution_setup_sender: bmrng::RequestSender<(PeerId, NewSwap), Result<State2>>,

    /// When a (PeerId, Uuid, EncryptedSignature) tuple is sent into this channel, the EventLoop will:
    /// 1. Send the encrypted signature to the specified peer over the network
    /// 2. Return Ok(()) if the peer acknowledges receipt, or
    /// 3. Return an OutboundFailure error if the request fails
    encrypted_signature_sender:
        bmrng::RequestSender<(PeerId, Uuid, EncryptedSignature), Result<(), OutboundFailure>>,

    /// When a PeerId is sent into this channel, the EventLoop will:
    /// 1. Request a price quote from the specified peer
    /// 2. Return the quote if successful
    /// 3. Return an OutboundFailure error if the request fails
    quote_sender: bmrng::RequestSender<PeerId, Result<BidQuote, OutboundFailure>>,

    /// When a (PeerId, Uuid) tuple is sent into this channel, the EventLoop will:
    /// 1. Request the specified peer's cooperation in redeeming the Monero for the given swap
    /// 2. Return a response object (Fullfilled or Rejected), if the network request is successful
    ///    The Fullfilled object contains the keys required to redeem the Monero
    /// 3. Return an OutboundFailure error if the network request fails
    cooperative_xmr_redeem_sender: bmrng::RequestSender<
        (PeerId, Uuid),
        Result<cooperative_xmr_redeem_after_punish::Response, OutboundFailure>,
    >,

    queued_transfer_proof_sender: bmrng::RequestSender<
        (
            Uuid,
            PeerId,
            bmrng::RequestSender<monero::TransferProof, ()>,
        ),
        (),
    >,
}

impl EventLoopHandle {
    fn create_retry_config(max_elapsed_time: Duration) -> backoff::ExponentialBackoff {
        backoff::ExponentialBackoffBuilder::new()
            .with_max_elapsed_time(max_elapsed_time.into())
            .with_max_interval(Duration::from_secs(5))
            .build()
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
        let (transfer_proof_sender, transfer_proof_receiver) = bmrng::channel(1);

        // Register this sender in the `EventLoop`
        // It is put into the queue and then later moved into `registered_transfer_proof_senders`
        //
        // We use `send(...) instead of send_receive(...)` because the event loop needs to be running for this to respond
        self.queued_transfer_proof_sender
            .send((swap_id, peer_id, transfer_proof_sender))
            .await
            .context("Failed to register transfer proof sender with event loop")?;

        Ok(SwapEventLoopHandle {
            handle: self.clone(),
            peer_id,
            swap_id,
            transfer_proof_receiver: Some(transfer_proof_receiver),
        })
    }

    pub async fn setup_swap(&mut self, peer_id: PeerId, swap: NewSwap) -> Result<State2> {
        tracing::debug!(swap = ?swap, %peer_id, "Sending swap setup request");

        let backoff = Self::create_retry_config(EXECUTION_SETUP_PROTOCOL_TIMEOUT);

        backoff::future::retry_notify(backoff, || async {
            match self.execution_setup_sender.send_receive((peer_id, swap.clone())).await {
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

    pub async fn request_quote(&mut self, peer_id: PeerId) -> Result<BidQuote> {
        tracing::debug!(%peer_id, "Requesting quote");

        let backoff = Self::create_retry_config(REQUEST_RESPONSE_PROTOCOL_TIMEOUT);

        backoff::future::retry_notify(backoff, || async {
            match self.quote_sender.send_receive(peer_id).await {
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

    pub async fn request_cooperative_xmr_redeem(
        &mut self,
        peer_id: PeerId,
        swap_id: Uuid,
    ) -> Result<Response> {
        tracing::debug!(%peer_id, %swap_id, "Requesting cooperative XMR redeem");

        let backoff = Self::create_retry_config(REQUEST_RESPONSE_PROTOCOL_TIMEOUT);

        backoff::future::retry_notify(backoff, || async {
            match self.cooperative_xmr_redeem_sender.send_receive((peer_id, swap_id)).await {
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

    pub async fn send_encrypted_signature(
        &mut self,
        peer_id: PeerId,
        swap_id: Uuid,
        tx_redeem_encsig: EncryptedSignature,
    ) -> Result<()> {
        tracing::debug!(%peer_id, %swap_id, "Sending encrypted signature");

        // We will retry indefinitely until we succeed
        let backoff = backoff::ExponentialBackoffBuilder::new()
            .with_max_elapsed_time(None)
            .with_max_interval(REQUEST_RESPONSE_PROTOCOL_TIMEOUT)
            .build();

        backoff::future::retry_notify(backoff, || async {
            match self.encrypted_signature_sender.send_receive((peer_id, swap_id, tx_redeem_encsig.clone())).await {
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
        .context("Failed to send encrypted signature after retries")
    }
}

#[derive(Debug)]
pub struct SwapEventLoopHandle {
    handle: EventLoopHandle,
    peer_id: PeerId,
    swap_id: Uuid,
    transfer_proof_receiver: Option<bmrng::RequestReceiver<monero::TransferProof, ()>>,
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

    pub async fn send_encrypted_signature(
        &mut self,
        tx_redeem_encsig: EncryptedSignature,
    ) -> Result<()> {
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
