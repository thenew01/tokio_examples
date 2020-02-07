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
use std::str;
use std::str::FromStr;
use tokio::net::{TcpListener, TcpStream};
use tokio::stream::{Stream, StreamExt};
use tokio::sync::{mpsc, Mutex};
use tokio_util::codec::{Framed, BytesCodec, LengthDelimitedCodec, LengthDelimitedCodecError};

use futures::SinkExt;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::io;
use std::net::SocketAddr;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use bytes::Bytes;
use bytes::BytesMut;
//use tini::Ini;
use ini::Ini;



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

    let ip : Vec<&str> = addr.split(':').collect();
    let ip = ip[0];
    let local_addr = IpAddr::from_str(&ip).unwrap();

    let mut client_id : i32 = 0;
    let mut peer_id : i32 = -1;
    loop {
        // Asynchronously wait for an inbound TcpStream.
        let (stream, _addr) = listener.accept().await?;

        // Clone a handle to the `Shared` state for the new connection.
        let state = Arc::clone(&state);

        client_id += 1;
        peer_id += 1;
        let peer_id2 = peer_id.clone();
        let mut peer_id3 = peer_id.clone();

        let mut is_server = false;
        if peer_id3 == 0 {
            is_server = true;
        }

        // Spawn our handler to be run asynchronously.
        tokio::spawn(async move {
            if is_server == false {
                let mut buf : Vec<u8> = [0u8; 4].to_vec();
                /*buf[0] = 8;
                buf[1] = 0;
                buf[2] = 0;
                buf[3] = 0;*/
                buf[0]= ( peer_id2 & 0xff ) as u8;
                buf[1] = ( ( peer_id2 >> 8 ) & 0xff ) as u8;
                buf[2] = ( ( peer_id2 >> 16 ) & 0xff ) as u8;
                buf[3] = ( ( peer_id2 >> 24 ) & 0xff ) as u8;

                let mut state = state.lock().await;
                state.sendto_server(&_addr, &Bytes::from(buf)).await;
            }
            if let Err(e) = process(state, stream, local_addr, _addr, peer_id2, is_server).await {
                println!("an error occured; error = {:?}", e);
            }
        });
    }
}

/// Shorthand for the transmit half of the message channel.
type Tx = mpsc::UnboundedSender< Bytes>;

/// Shorthand for the receive half of the message channel.
type Rx = mpsc::UnboundedReceiver< Bytes >;

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
    frames: Framed<TcpStream,  LengthDelimitedCodec>,

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
//    async fn broadcast(&mut self, sender: SocketAddr, message: &Vec<u8>) {
//        for peer in self.peers.iter_mut() {
//            if *peer.0 != sender {
//                let _ = peer.1.send(message.to_vec());
//            }
//        }
//    }
    /*		async fn sendto_all_client_by_id(&mut self, message: Bytes) {
			for peer in self.peer_ids.iter_mut() {
				//if *peer.0 == id
				{
					let _ = peer.1.send(message.into());
				}
			}
		}*/
    async fn sendto_client(&mut self, client: SocketAddr, message: &Bytes) {
        for peer in self.peers.iter_mut() {
            //if *peer.0 == client
            {
                let _ = peer.1.send( *message) ;
            }
        }
    }
    async fn sendto_server(&mut self, server: &SocketAddr, message: &Bytes ) {
        for server in self.servers.iter_mut() {
            //if *server.0 == server
            {
                let _ = server.1.send( *message);
            }
        }
    }
    async fn sendto_client_by_id(&mut self, id: i32, message: &Bytes) {
        for peer in self.peer_ids.iter_mut() {
            if *peer.0 == id {
                let _ = peer.1.send( *message);
            }
        }
    }
}

impl Peer {
    /// Create a new instance of `Peer`.
    async fn new(
        state: Arc<Mutex<Shared>>,
        frames: Framed<TcpStream, LengthDelimitedCodec>,
        is_server : bool,
        client_id: i32,
    ) -> io::Result<Peer> {
        // Get the client socket address
        let addr = frames.get_ref().peer_addr()?;

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

        Ok(Peer { frames, rx, is_server })
    }
}

#[derive(Debug)]
enum Message {
    /// A message that should be broadcasted to others.
    Broadcast(Bytes ),

    TransferToClient(Bytes ),
    TransferToServer(Bytes),

    /// A message that should be received by a client
    Received(Bytes ),
}

// Peer implements `Stream` in a way that polls both the `Rx`, and `Framed` types.
// A message is produced whenever an event is ready until the `Framed` stream returns `None`.
impl Stream for Peer {
    type Item = Result<Message, ()>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // First poll the `UnboundedReceiver`.

        if let Poll::Ready(Some(v)) = Pin::new(&mut self.rx).poll_next(cx) {
            return Poll::Ready(Some(Ok(Message::Received(v))));
        }

        // Secondly poll the `Framed` stream.
        let result: Option<_> = futures::ready!(Pin::new(&mut self.frames).poll_next(cx));

        Poll::Ready(match result {
            // We've received a message
            Some(Ok(message)) => {
                //TODO!分服务器客户端分别处理
                if self.is_server {
                    //给客户端的消息
                    Some(Ok(Message::TransferToClient( Bytes::from(message.to_vec()))))
                }
                else {
                    //给服务器的消息
                    Some(Ok(Message::TransferToServer( Bytes::from(message.to_vec()))))
                }
                // We've received a message we should broadcast to others.
                //Some(Ok(Message::Broadcast(message.to_vec())))
            },

            // An error occured.
            //Some(Err(e)) => Some(Err(e)),
            //Some(Err(e)) => Some(Err( e.source().unwrap())),
            _ => None,

            // The stream has been exhausted.
            None => None,
        })
    }
}

/// Process an individual chat client
async fn process(
    state: Arc<Mutex<Shared>>,
    stream: TcpStream,
    local_addr: IpAddr,
    addr: SocketAddr,
    peer_id : i32,
    is_server : bool,
) -> Result<(), Box<dyn Error>> {
    let mut io_packet = Framed::new(stream, LengthDelimitedCodec::new());
    //let codec = LengthDelimitedCodec::builder().little_endian();
    //let mut io_packet = Framed::new(stream, LengthDelimitedCodec::);

    //TODO!另开线程监听服务器端口，由于线程与task通信暂时麻烦，就从配置文件判断是否是服务器ip了

    let mut server_name : String = "".to_string();
    let mut username = "";

//    if is_server {
//         // Read the first line from the `LineCodec` stream to get the username.
//         let mut packet = match io_packet.next().await {
//             Some(Ok(apacket)) => apacket,
//             // We didn't get a line so we return early here.
//             Some(Err(e)) => {
//                 println!("Failed to get msg from {}, reason: {}. Client disconnected.", addr, e);
//                 return Ok(());
//             },
//             None => BytesMut::new(),
//         };
//    }

    let is_server = is_server.clone();
    let is_server2 = is_server.clone();

    let peer_id2 = peer_id.clone();
    // Register our peer with state which internally sets up some channels.
    let mut peer = Peer::new(state.clone(), io_packet, is_server2, peer_id ).await?;

    // Process incoming messages until our stream is exhausted by a disconnect.
    while let Some(result) = peer.next().await {
        match result {
            // A message was received from the current user, we should
            // broadcast this message to the other users.
            Ok(Message::Broadcast(msg)) => {
                let mut state = state.lock().await;
                //let msg = format!("{}: {}", username, String::from_utf8(msg).unwrap());

                if is_server {
                    //state.broadcast(addr, &msg.into_bytes()).await;
                    //state.sendto_client_by_id(id,  &msg).await;
                }
                else {
                    //state.sendto_server(addr,  Bytes::from(msg)).await;
                }
            }
            Ok(Message::TransferToClient(msg)) => {
                let mut state = state.lock().await;
                //let msg = format!(" {}: {}", username, msg);

                assert_eq!(is_server, true);

                //get client_id from msg content
                if is_server {
                    //state.broadcast(addr, &msg).await;
                    let msgR = msg.clone();
                    let (client_id, msg0) = msg.split_at(4);
                    let sl = client_id;
                    let mut a : [u8; 4] = [sl[0], sl[1], sl[2], sl[3]];
                    let client_id0 = i32::from_le_bytes( a );
                    state.sendto_client_by_id( client_id0, &msgR ).await;
                }
            }
            Ok(Message::TransferToServer(msg)) => {
                let mut state = state.lock().await;
                //let msg = format!(" {}: {}", username, msg.into());
                assert_eq!(is_server, false);
                state.sendto_server(&addr,  &msg ).await;
            }


            // A message was received from a peer. Send it to the
            // current user.
            Ok(Message::Received(msg)) => {
                println!("recv is_server {},peer.is_server {} ", &is_server, &peer.is_server);

                peer.frames.send( msg ).await?;
            }
            Err(e) => {
                println!( "an error occured while processing messages for {}; error = {:?}",username, e);
            }
        }
    }

    // If this section is reached it means that the client was disconnected!
    // Let's let everyone still connected know about it.
    {
        let mut state = state.lock().await;
        state.peers.remove(&addr);

        let msg = format!("{} has left the chat", peer_id2);
        println!("{}", msg);
        //state.broadcast(addr,  Bytes::from(msg)).await;
    }

    Ok(())
}
