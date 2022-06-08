//! A chat server that broadcasts a message to all connections,
//! originated from tokio example.
//
//! update to tracing future and discovery future spawn/run logic.
//!
//! This example is explicitly more verbose than it has to be. This is to
//! illustrate more concepts.
//!
//! A chat server for telnet clients. After a telnet client connects, the first
//! line should contain the client's name. After that, all lines sent by a
//! client are broadcasted to all other connected clients.
//!
//! Because the client is telnet, lines are delimited by "\r\n".
//!
//! You can test this out by running:
//!
//!     cargo run --example chat
//!
//! And then in another terminal run:
//!
//!     telnet localhost 6142
//!
//! You can run the `telnet` command in any number of additional windows.
//!
//! You can run the second command in multiple windows and then chat between the
//! two, seeing the messages from the other client as they're received. For all
//! connected clients they'll all join the same room and see everyone else's
//! messages.

#![warn(rust_2018_idioms)]
#![feature(trace_macros)]

use tokio::net::{TcpListener, TcpStream};
use tokio::stream::{Stream, StreamExt};
use tokio::sync::{mpsc, Mutex};
use tokio_util::codec::{Framed, LinesCodec, LinesCodecError};

use futures::SinkExt;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tracing::{debug_span, span, Level};
use tracing_futures::Instrument as OthInstrument;
use tracing_attributes::instrument;
use tracing_subscriber::{prelude::*, registry::Registry};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    setup_global_subscriber();

    // Create the shared state. This is how all the peers communicate.
    //
    // The server task will hold a handle to this. For every new client, the
    // `state` handle is cloned and passed into the task that processes the
    // client connection.
    let state = Arc::new(Mutex::new(Shared::new()));

    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:6142".to_string());

    // Bind a TCP listener to the socket address.
    //
    // Note that this is the Tokio TcpListener, which is fully async.
    let mut listener = TcpListener::bind(&addr).await?;

    tracing::info!("server running on {}", addr);
    println!("server running on {}", addr);

    loop {
        // Asynchronously wait for an inbound TcpStream.
        let (stream, addr) = listener.accept().await?;
        span!(Level::INFO, "accept").in_scope(|| {
            // Clone a handle to the `Shared` state for the new connection.
            let state = Arc::clone(&state);
            // Spawn our handler to be run asynchronously.
            // trace_macros!(true);
            span!(Level::ERROR, "spawn").in_scope(|| {
                tokio::spawn(async move {
                    tracing::info!("accepted connection");
                    println!("accepted connection");
                    if let Err(e) = process(state, stream, addr).await {
                        tracing::info!("an error occurred; error = {:?}", e);
                        println!("an error occurred; error = {:?}", e);
                    }
                });
            });
            // trace_macros!(false);
        });
    }
}

fn setup_global_subscriber() {
    let _layer = tracing_libatrace::layer()
        .unwrap()
        .with_data_field(Option::Some("data".to_string()));
    let subscriber = Registry::default().with(_layer);
    tracing::subscriber::set_global_default(subscriber).unwrap();
}

/// Shorthand for the transmit half of the message channel.
type Tx = mpsc::UnboundedSender<String>;

/// Shorthand for the receive half of the message channel.
type Rx = mpsc::UnboundedReceiver<String>;

/// Data that is shared between all peers in the chat server.
///
/// This is the set of `Tx` handles for all connected clients. Whenever a
/// message is received from a client, it is broadcasted to all peers by
/// iterating over the `peers` entries and sending a copy of the message on each
/// `Tx`.
struct Shared {
    peers: HashMap<SocketAddr, Tx>,
}

/// The state for each connected client.
struct Peer {
    /// The TCP socket wrapped with the `Lines` codec, defined below.
    ///
    /// This handles sending and receiving data on the socket. When using
    /// `Lines`, we can work at the line level instead of having to manage the
    /// raw byte operations.
    lines: Framed<TcpStream, LinesCodec>,

    /// Receive half of the message channel.
    ///
    /// This is used to receive messages from peers. When a message is received
    /// off of this `Rx`, it will be written to the socket.
    rx: Rx,
}

impl Shared {
    /// Create a new, empty, instance of `Shared`.
    fn new() -> Self {
        Shared {
            peers: HashMap::new(),
        }
    }

    /// Send a `LineCodec` encoded message to every peer, except
    /// for the sender.
    async fn broadcast(&mut self, sender: SocketAddr, message: &str) {
        for peer in self.peers.iter_mut() {
            if *peer.0 != sender {
                tracing::info!("send:{}, msg:{}", peer.0, message);
                let _ = peer.1.send(message.into());
            }
        }
    }
}

impl Peer {
    /// Create a new instance of `Peer`.
    async fn new(
        state: Arc<Mutex<Shared>>,
        lines: Framed<TcpStream, LinesCodec>,
    ) -> io::Result<Peer> {
        // Get the client socket address
        let addr = lines.get_ref().peer_addr()?;

        // Create a channel for this peer
        let (tx, rx) = mpsc::unbounded_channel();

        // Add an entry for this `Peer` in the shared state map.
        state.lock().await.peers.insert(addr, tx);

        Ok(Peer { lines, rx })
    }
}

#[derive(Debug)]
enum Message {
    /// A message that should be broadcasted to others.
    Broadcast(String),

    /// A message that should be received by a client
    Received(String),
}

// Peer implements `Stream` in a way that polls both the `Rx`, and `Framed` types.
// A message is produced whenever an event is ready until the `Framed` stream returns `None`.
impl Stream for Peer {
    type Item = Result<Message, LinesCodecError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // First poll the `UnboundedReceiver`.

        if let Poll::Ready(Some(v)) = Pin::new(&mut self.rx).poll_next(cx) {
            return Poll::Ready(Some(Ok(Message::Received(v))));
        }

        // Secondly poll the `Framed` stream.
        let result: Option<_> = futures::ready!(Pin::new(&mut self.lines).poll_next(cx));

        Poll::Ready(match result {
            // We've received a message we should broadcast to others.
            Some(Ok(message)) => Some(Ok(Message::Broadcast(message))),

            // An error occurred.
            Some(Err(e)) => Some(Err(e)),

            // The stream has been exhausted.
            None => None,
        })
    }
}

/// Process an individual chat client
#[instrument(skip(state))]
async fn process(
    state: Arc<Mutex<Shared>>,
    stream: TcpStream,
    addr: SocketAddr,
) -> Result<(), Box<dyn Error>> {
    let mut lines = Framed::new(stream, LinesCodec::new());
    // Send a prompt to the client to enter their username.
    lines
        .send("Please enter your username:".to_string())
        .instrument(debug_span!("send_username", __fut = 0))
        .await?;

    // Read the first line from the `LineCodec` stream to get the username.
    let username = match lines
        .next()
        .instrument(debug_span!("get_usename", __fut = 0, data = ""))
        .await
    {
        Some(Ok(line)) => line,
        // We didn't get a line so we return early here.
        _ => {
            tracing::error!("Failed to get username from {}. Client disconnected.", addr);
            return Ok(());
        }
    };

    // Register our peer with state which internally sets up some channels.
    let mut peer = Peer::new(state.clone(), lines)
        .instrument(debug_span!("new_peer", __fut = 0))
        .await?;

    // A client has connected, let's let everyone know.
    {
        let mut state = state.lock().instrument(debug_span!("lock state")).await;
        let msg = format!("{} has joined the chat", username);
        tracing::info!("{}", msg);
        println!("{}", msg);
        state
            .broadcast(addr, &msg)
            .instrument(debug_span!("broadcast_newuser"))
            .await;
    }

    // Process incoming messages until our stream is exhausted by a disconnect.
    while let Some(result) = peer
        .next()
        .instrument(debug_span!("peer_incoming", __fut = ""))
        .await
    {
        match result {
            // A message was received from the current user, we should
            // broadcast this message to the other users.
            Ok(Message::Broadcast(msg)) => {
                let mut state = state.lock().instrument(debug_span!("lock state.bm")).await;
                let msg = format!("{}: {}", username, msg);
                tracing::info!("bc msg:from {} {}", username, msg);
                state
                    .broadcast(addr, &msg)
                    .instrument(debug_span!("bc_msg"))
                    .await;
            }
            // A message was received from a peer. Send it to the
            // current user.
            Ok(Message::Received(msg)) => {
                peer.lines
                    .send(msg)
                    .instrument(debug_span!("sendmsg_peer"))
                    .await?;
            }
            Err(e) => {
                tracing::error!(
                    "an error occurred while processing messages for {}; error = {:?}",
                    username,
                    e
                );
            }
        }
    }

    // If this section is reached it means that the client was disconnected!
    // Let's let everyone still connected know about it.
    {
        let mut state = state.lock().instrument(debug_span!("lock state.dis")).await;
        state.peers.remove(&addr);

        let msg = format!("{} has left the chat", username);
        tracing::info!("{}", msg);
        println!("{}", msg);
        state
            .broadcast(addr, &msg)
            .instrument(debug_span!("bc_user left"))
            .await;
    }

    Ok(())
}
