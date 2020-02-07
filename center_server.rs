//! A chat server that broadcasts a message to all connections.
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Create the shared state. This is how all the peers communicate.
    //
    // The server task will hold a handle to this. For every new client, the
    // `state` handle is cloned and passed into the task that processes the
    // client connection.
    let state = Arc::new(Mutex::new(Shared::new()));

    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8080".to_string());

    // Bind a TCP listener to the socket address.
    //
    // Note that this is the Tokio TcpListener, which is fully async.
    let mut listener = TcpListener::bind(&addr).await?;

    println!("server running on {}", addr);

    let mut client_id = 0;
    loop {
        // Asynchronously wait for an inbound TcpStream.
        let (stream, addr) = listener.accept().await?;

        // Clone a handle to the `Shared` state for the new connection.
        let state = Arc::clone(&state);

        client_id += 1;
        let id =  client_id.clone();

        // Spawn our handler to be run asynchronously.
        tokio::spawn(async move {
            if let Err(e) = process(state, stream, addr, id).await {
                println!("an error occured; error = {:?}", e);
            }
        });
    }
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
    peer_ids : HashMap<i32, Tx>,
    servers : HashMap<SocketAddr, Tx>,
}

/// The state for each connected client.
struct Peer {
    /// The TCP socket wrapped with the `Lines` codec, defined below.
    ///
    /// This handles sending and receiving data on the socket. When using
    /// `Lines`, we can work at the line level instead of having to manage the
    /// raw byte operations.
    lines: Framed<TcpStream,  LinesCodec>,

    /// Receive half of the message channel.
    ///
    /// This is used to receive messages from peers. When a message is received
    /// off of this `Rx`, it will be written to the socket.
    rx: Rx,
    is_server : bool,
}

impl Shared {
    /// Create a new, empty, instance of `Shared`.
    fn new() -> Self {
        Shared {
            peers: HashMap::new(),
            peer_ids : HashMap:: new(),
            servers : HashMap::new(),
        }
    }

    /// Send a `LineCodec` encoded message to every peer, except
    /// for the sender.
    async fn broadcast(&mut self, sender: SocketAddr, message: &str) {
        for peer in self.peers.iter_mut() {
            if *peer.0 != sender {
                let _ = peer.1.send(message.into());
            }
        }
    }

    async fn sendto_server(&mut self, server: SocketAddr, message: &str) {
        for server in self.servers.iter_mut() {
            //if *server.0 == server
            {
                let _ = server.1.send(message.into());
            }
        }
    }

    async fn sendto_client(&mut self, client: SocketAddr, message: &str) {
        for peer in self.peers.iter_mut() {
            //if *peer.0 == client
            {
                let _ = peer.1.send(message.into());
            }
        }
    }
    async fn sendto_client_by_id(&mut self, id: i32, message: &str) {
        for peer in self.peer_ids.iter_mut() {
            if *peer.0 == id {
                let _ = peer.1.send(message.into());
            }
        }
    }
    async fn sendto_all_client_by_id(&mut self, message: &str) {
        for peer in self.peer_ids.iter_mut() {
            //if *peer.0 == id
            {
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
        is_server : bool,
        client_id: i32,
    ) -> io::Result<Peer> {
        // Get the client socket address
        let addr = lines.get_ref().peer_addr()?;

        // Create a channel for this peer
        let (tx, rx) = mpsc::unbounded_channel();

        // Add an entry for this `Peer` in the shared state map.
        if is_server {
            state.lock().await.servers.insert(addr, tx);
        }
        else {
            //state.lock().await.peers.insert(addr, tx);
            state.lock().await.peer_ids.insert(client_id, tx);
        }

        Ok(Peer { lines, rx, is_server })
    }
}

#[derive(Debug)]
enum Message {
    /// A message that should be broadcasted to others.
    Broadcast(String),

    TransferToClient(String),
    TransferToServer(String),

    /// A message that should be received by a client
    Received(String),
}

// Peer implements `Stream` in a way that polls both the `Rx`, and `Framed` types.
// A message is produced whenever an event is ready until the `Framed` stream returns `None`.
impl Stream for Peer {
    type Item = Result<Message, LinesCodecError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // First poll the `UnboundedReceiver`.
        //task之间通信
        if let Poll::Ready(Some(v)) = Pin::new(&mut self.rx).poll_next(cx) {
            return Poll::Ready(Some(Ok(Message::Received(v))));
        }

        // Secondly poll the `Framed` stream.
        //网络通信
        let result: Option<_> = futures::ready!(Pin::new(&mut self.lines).poll_next(cx));

        Poll::Ready(match result {
            // We've received a message we should broadcast to others.
            Some(Ok(message)) => {
                //TODO分服务器客户端分别处理
                if self.is_server {
                    //给客户端的消息
                    Some(Ok(Message::TransferToClient(message)))
                }
                else {
                    //给服务器的消息
                    Some(Ok(Message::TransferToServer(message)))
                }
                //Some(Ok(Message::Broadcast(message)))
            },

            // An error occured.
            Some(Err(e)) => Some(Err(e)),

            // The stream has been exhausted.
            None => None,
        })
    }
}

/// Process an individual chat client
async fn process(
    state: Arc<Mutex<Shared>>,
    stream: TcpStream,
    addr: SocketAddr,
    id : i32,
) -> Result<(), Box<dyn Error>> {
    let mut lines = Framed::new(stream, LinesCodec::new());

    // Send a prompt to the client to enter their username.
    lines
        .send(String::from("Please enter your username:"))
        .await?;

    // Read the first line from the `LineCodec` stream to get the username.
    let segs = match lines.next().await {
        Some(Ok(line)) => line,
        // We didn't get a line so we return early here.
        _ => {
            println!("Failed to get username from {}. Client disconnected.", addr);
            return Ok(());
        }
    };

    let mut is_server = false;
    let v : Vec<&str> = segs.split_whitespace().collect();
    let first_seg = v[0];
    let second_seg = v[1];
    let mut server_name = "";
    let mut username = "";
    if first_seg == "is_server" {
        is_server = true;
    }
    else {
        username = first_seg;
    }

    let is_server2 = is_server.clone();

    if !is_server {
        server_name = second_seg;
    }
    else {
        server_name = second_seg;
        username = second_seg;
    }
    // Register our peer with state which internally sets up some channels.
    let mut peer = Peer::new(state.clone(), lines, is_server, id ).await?;

    let id = id.clone();

    // A client has connected, let's let everyone know.
    if !is_server2 { //发给服务器
        let mut state = state.lock().await;

        let msg = format!("{} has joined the chat, send msg to {}", username, server_name);
        println!("{}", msg);

        //state.broadcast(addr, &msg).await;
        state.sendto_server(addr, &msg).await;
    }
    else { // 发给客户端
        let mut state = state.lock().await;

        let msg = format!("{} has joined the chat， is server {} ", username, server_name);
        println!("{}", msg);

        //state.broadcast(addr, &msg).await;
        //state.sendto_client(addr, &msg).await;
        state.sendto_client_by_id(id, &msg).await;
    }


    // Process incoming messages until our stream is exhausted by a disconnect.
    //while let Some(result) = peer.next().await {
    loop {
        if let Some(result) = peer.next().await {
            let test = 0;

            match result {
                // A message was received from the current user, we should
                // broadcast this message to the other users.
                Ok(Message::Broadcast(msg)) => {
                    let mut state = state.lock().await;
                    let msg = format!("{}: {}", username, msg);

                    if is_server {
                        //state.broadcast(addr, &msg).await;
                        state.sendto_client_by_id(id, &msg).await;
                    } else {
                        state.sendto_server(addr, &msg).await;
                    }
                }
                Ok(Message::TransferToClient(msg)) => {
                    let mut state = state.lock().await;
                    let msg = format!(" {}: {}", username, msg);

                    assert_eq!(is_server, true);

                    //get client_id from msg content
                    if is_server {
                        //state.broadcast(addr, &msg).await;
                        state.sendto_all_client_by_id(&msg).await;
                    }
                }
                Ok(Message::TransferToServer(msg)) => {
                    let mut state = state.lock().await;
                    let msg = format!(" {}: {}", username, msg);

                    assert_eq!(is_server, false);

                    state.sendto_server(addr, &msg).await;
                }

                // A message was received from a peer. Send it to the
                // current user.
                Ok(Message::Received(msg)) => {
                    println!("recv is_server {},peer.is_server {} ", &is_server, &peer.is_server);
                    peer.lines.send(msg).await?;
                }
                Err(e) => {
                    println!(
                        "an error occured while processing messages for {}; error = {:?}",
                        username, e
                    );
                }
            }
        }
    }
    // If this section is reached it means that the client was disconnected!
    // Let's let everyone still connected know about it.
    {
        let mut state = state.lock().await;
        state.peers.remove(&addr);

        let msg = format!("{} has left the chat", username);
        println!("{}", msg);
        state.broadcast(addr, &msg).await;
    }

    Ok(())
}
