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
use std::net::SocketAddr;
use std::net::{IpAddr,  Shutdown};
use std::time::Duration;
use tokio::stream::{Stream, StreamExt};
use tokio::sync::{mpsc, Mutex};
use tokio_util::codec::{Framed,  LengthDelimitedCodec, Builder, LengthDelimitedCodecError};

use futures::SinkExt;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::io;

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use bytes::Bytes;
//use bytes::BytesMut;
////use tini::Ini;
use ini::Ini;

//use std::intrinsics::size_of;
//use crate::msg_from_client::data_to_server;

use console::Term;

#[macro_use]
extern crate log;
extern crate simple_logger;
extern crate simplelog;

use simplelog::*;

use std::fs::File;

enum msg_server
{
    ID_CLIENT_CONNECTED = 20001,
    ID_CLIENT_DISCONNECTED = 20002,
    ID_CLIENT_DATA = 20003,
    ID_REGISTER_RESPONSE = 20004,
}

enum msg_client
{
    ID_DATA_EVENT = 100001,
    ID_SERVER_DISCONNECTED = 100004,
}

fn encode_head(src : &mut Vec<u8> ) -> Vec<u8> {
    src[0] = ( src[0] ^ 0xcf ) & 0xff;
    src[1] = ( src[1] ^ 0xcf ) & 0xff;
    src.to_vec()
}

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
    //simple_logger::init().unwrap();

    //warn!("This is an example message.");
    CombinedLogger::init(
        vec![
            TermLogger::new(LevelFilter::Warn,
                            ConfigBuilder::new().set_time_format_str("%Y-%m-%d %H:%M:%S").build(),
                            TerminalMode::Mixed).unwrap(),
            WriteLogger::new(LevelFilter::Info,
                             ConfigBuilder::new().set_time_format_str("%Y-%m-%d %H:%M:%S").build(),
                             File::create("net.log").unwrap()),
        ]
    ).unwrap();

    //error!("Bright red error\n");
    warn!("gate started");
    //info!("gate started");
    //debug!("This level is currently not enabled for any logger\n");

    let term = Term::stdout();
    //term.set_title()

    let ip : Vec<&str> = addr.split(':').collect();
    let ip = ip[0];
    let local_addr = IpAddr::from_str(&ip).unwrap();

   /*let mut server_ip: String;
    let mut server_port: String = "".to_string();
    let i = Ini::load_from_file("conf.ini").unwrap();
    for (sec, prop) in i.iter() {
        println!("Section: {:?}", sec);
        for (k, v) in prop.iter() {
            println!("{}:{}", k, v);
            if k == "server_ip" {
                server_ip = v.clone();
            }
            else if k == "server_port"{
                //let vv = v.clone();
                //let port = vv.into_bytes();
                server_port = v.clone();//port[0] as u32| ( port[1] << 8 ) as u32 | (port[2] << 16) as u32 | (port[3] << 24) as u32;
                //println!("port {}, server_port {} ", v, server_port);
            }
        }
    }
*/

    let mut client_id : i64 = 0;
    let mut peer_id : i64 = -1;
    loop {
        // Asynchronously wait for an inbound TcpStream.
        let (stream, _addr) = listener.accept().await?;

        //stream.set_nonblocking(true);
        stream.set_nodelay(true);
        //stream.set_linger(Some( Duration::new(1,0)));
        stream.set_keepalive(Some(Duration::new(60*5, 0)));

        // Clone a handle to the `Shared` state for the new connection.
        let state = Arc::clone(&state);

        client_id += 1;

        //let client_num = client_id.clone();
        //term.set_title(client_num);

        peer_id += 1;
        let peer_id2: i64 = peer_id.clone();
        let mut peer_id3 = peer_id.clone();

        warn!("session [{}] {} has connected", peer_id, _addr);

        let mut is_server = false;
        if peer_id == 0 {
            //if _addr.port().to_string() == server_port {
            is_server = true;
            warn!("server is incoming");
        }

        // Spawn our handler to be run asynchronously.
        tokio::spawn(async move {
            if is_server == false {
                let mut buf : Vec<u8> = [0u8; 12].to_vec();
                //client incoming
                buf[0] = ( peer_id2 & 0xff ) as u8;
                buf[1] = ( ( peer_id2 >> 8 ) & 0xff ) as u8;
                buf[2] = ( ( peer_id2 >> 16 ) & 0xff ) as u8;
                buf[3] = ( ( peer_id2 >> 24 ) & 0xff ) as u8;
                buf[4] = ( ( peer_id2 >> 32 ) & 0xff ) as u8;
                buf[5] = ( ( peer_id2 >> 40 ) & 0xff ) as u8;
                buf[6] = ( ( peer_id2 >> 48 ) & 0xff ) as u8;
                buf[7] = ( ( peer_id2 >> 56 ) & 0xff ) as u8;

                let msg_type = 20001;
                buf[8] = ( ( msg_type >> 0 ) & 0xff ) as u8;
                buf[9] = ( ( msg_type >> 8 ) & 0xff ) as u8;
                buf[10] = ( ( msg_type >> 16 ) & 0xff ) as u8;
                buf[11] = ( ( msg_type >> 24 ) & 0xff ) as u8;

                /*
                let packet_len = (8 + 4 + 2) as i16;
                let len : [u8; 2] = packet_len.to_le_bytes();
                let mut msg_r = String::from_utf8_lossy(&len[0..2]).to_string();
                msg_r.push_str(&String::from_utf8_lossy(&buf[0..12]).to_string() );
                */
                let ip = _addr.ip().to_string();

                let mut msg_r = unsafe { String::from_utf8_unchecked(buf ) };
                msg_r.push_str(&ip);
                let mut state = state.lock().await;
                state.sendto_server(&_addr, &msg_r ).await;
            }
            if let Err(e) = process(state, stream, local_addr, _addr, peer_id3, is_server).await {
                warn!("an error occurred; 000 !!!! connection {} {} error = {:?}", peer_id3, _addr, e);
                // let peer_id = peer_id.clone();
                // if !is_server == false {
                //     notify_server_client_disconnected(&peer_id, &state, &_addr).await;
                // }
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
    peer_ids : HashMap<i64, Tx>,
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

    /// Send a message to every clients
    async fn broadcast(&mut self, message: &String) {
        for peer in self.peer_ids.iter_mut() {
            //if *peer.0 == id
            {
                let _ = peer.1.send(message.to_string());
            }
        }
    }

    async fn sendto_server(&mut self, server: &SocketAddr, message: &String) {
        for server in self.servers.iter_mut() {
            //if *server.0 == server
            {
                let _ = server.1.send(message.to_string());
            }
        }
    }
    async fn sendto_client_by_id(&mut self, id: i64, message: &String) {
        for peer in self.peer_ids.iter_mut() {
            if *peer.0 == id {
                let _ = peer.1.send(message.to_string());
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
        client_id: i64,
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
            //let txSend = tx.clone();
            //state.lock().await.peers.insert(addr, txSend);
            state.lock().await.peer_ids.insert(client_id, tx);
        }

        Ok(Peer { frames, rx, is_server })
    }
}

#[derive(Debug)]
enum Message {
    /// A message that should be broadcasted to others.
    Broadcast(String),

    FromServer(String),
    FromClient(String),

    /// A message that should be received by a client
    Received(String),
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
                    Some(Ok(Message::FromServer( unsafe{ String::from_utf8_unchecked(message.to_vec()) }.to_string())))
                }
                else {
                    //给服务器的消息
                    Some(Ok(Message::FromClient( unsafe { String::from_utf8_unchecked(message.to_vec()) }.to_string())))
                }
                // We've received a message we should broadcast to others.
                //Some(Ok(Message::Broadcast(message.to_vec())))
            },

            // An error occured.
            //Some(Err(e)) => Some(Err(e)),
            //Some(Err(e)) => Some(Err( e.source().unwrap())),
            _ => None,

            // The stream has been exhausted.
            //None => None,
        })
    }
}

/// Process an individual chat client
async fn process(
    state: Arc<Mutex<Shared>>,
    stream: TcpStream,
    local_addr: IpAddr,
    addr: SocketAddr,
    peer_id : i64,
    is_server : bool,
) -> Result<(), Box<dyn Error>> {
    let mut builder : Builder = Builder::new();
    builder.little_endian();
    builder.length_field_length(2);
    builder.length_adjustment(-2);
    
    if !is_server{
        builder.first_packet_no_length_field(true);
        builder.is_server(false);
    }
    else{
        //builder.first_packet_no_length_field(true);
        builder.is_server(true);
    }

    //stream.shutdown(Shutdown::Both);
    //builder.encoded(true);

    //let mut io_packet = Framed::new(stream, LengthDelimitedCodec::new());
    let mut io_packet = Framed::new(stream, LengthDelimitedCodec::new_from_builder(builder));
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

    let peer_id2 : i64 = peer_id.clone();
    let peer_id0 = peer_id.clone();
    // Register our peer with state which internally sets up some channels.
    let mut peer = Peer::new(state.clone(), io_packet, is_server2, peer_id0 ).await?;

    let first_packet_to_client = true; //first packet is the random seed, a int, no length field

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
            Ok(Message::FromServer(msg)) => {
                let mut state = state.lock().await;
                //let msg = format!(" {}: {}", username, msg);

                assert_eq!(is_server, true);

                //get client_id from msg content
                if is_server {
                    //state.broadcast(addr, &msg).await;
                    let mut msg_r = msg.into_bytes();
                    let id = [msg_r[0], msg_r[1], msg_r[2], msg_r[3], msg_r[4], msg_r[5], msg_r[6], msg_r[7]];
                    let client_id0= i64::from_le_bytes( id );

                    //在encoder里不额外添加包头发给客户端
                    let mut msg_r2 = "".to_string();
                    msg_r2.push_str(unsafe { &String::from_utf8_unchecked(msg_r[8..].to_vec()) } );
                    state.sendto_client_by_id( client_id0, &msg_r2).await;
                }
            }
            Ok(Message::FromClient(msg)) => {
                let mut state = state.lock().await;

                assert_eq!(is_server, false);

                let client_id = peer_id.clone();

                let mut buf : Vec<u8> = [0u8; 12].to_vec();
                buf[0]= ( client_id & 0xff ) as u8;
                buf[1] = ( ( client_id >> 8 ) & 0xff ) as u8;
                buf[2] = ( ( client_id >> 16 ) & 0xff ) as u8;
                buf[3] = ( ( client_id >> 24 ) & 0xff ) as u8;
                buf[4] = ( ( client_id >> 32 ) & 0xff ) as u8;
                buf[5] = ( ( client_id >> 40 ) & 0xff ) as u8;
                buf[6] = ( ( client_id >> 48 ) & 0xff ) as u8;
                buf[7] = ( ( client_id >> 56 ) & 0xff ) as u8;

                let msg_type = 20003;
                buf[8] = ( msg_type & 0xff ) as u8;
                buf[9] = ((msg_type >> 8 ) & 0xff) as u8;
                buf[10] = ((msg_type >> 16 ) & 0xff) as u8;
                buf[11] = ((msg_type >> 24 ) & 0xff) as u8;

                let mut msg_r = unsafe { String::from_utf8_unchecked(buf ) };
                //println!("to server1: {} ", len);
                let mut len = msg.len()+2;  //+2 length field length decode时去除了包头（长度），因此这里要加上再给服务器
                let mut len = vec!( (len & 0xff) as u8, ( ( len >> 8 ) & 0xff) as u8 );
                let len2 = encode_head( &mut len ); //在length_delimiter里调用了一次是解密，再次则是加密

                //println!("to server2: {}", len2[0] | len2[1]);

                let len5 = unsafe { String::from_utf8_unchecked(len2) } ; //如果不是有效utf8，则会分配并替换为有效的utf8字符
                msg_r.push_str( &len5 );
                msg_r.push_str(&msg[..]);

                state.sendto_server(&addr,  &msg_r ).await;
            }

            // A message was received from a peer. Send it to the
            // current user.
            Ok(Message::Received(msg)) => {
                //println!("recv is_server {},peer.is_server {} ", &is_server, &peer.is_server);
                if msg.len() == 0 {
                    if !is_server {
                        warn!("close the client [{}] {} because the server is disconnected", peer_id, addr);
                    }
                    peer.frames.into_inner().shutdown(Shutdown::Both );
                    break;
                }
                if is_server == false {
                    let inner_msg_type = unsafe { msg.get_unchecked(4..5) };
                    let inner_msg_type= String::from(inner_msg_type).into_bytes();
                    let inner_msg_type =  ( inner_msg_type[0] ^ 0xcf ) & 0xff;
                    if inner_msg_type == 3 || inner_msg_type == 14 {
                        warn!("connection [{}] {} closed, because recv MSG_CLOSE or MSG_DISCONNECT", peer_id, addr);
                        let mut state = state.lock().await;
                        let client_id = peer_id.clone();
                        state.peer_ids.remove(&client_id);
                        peer.frames.close().await;
                        peer.frames.into_inner().shutdown(Shutdown::Both);
                        return Ok(());
                    }
                }

                if let Err(e) = peer.frames.send(Bytes::from(msg)).await {
                    warn!("connection {} {} closed send failed ", peer_id, addr);
                    let mut state = state.lock().await;
                    let client_id = peer_id.clone();
                    state.peer_ids.remove(&client_id);
                    peer.frames.close().await;
                    peer.frames.into_inner().shutdown(Shutdown::Both);
                    return Err(Box::<dyn Error>::from(e));
                };

            }
            Err(e) => {
                warn!( "an error occurred while processing messages for {}; error = {:?}",username, e);

                notify_server_client_disconnected(&peer_id, &state, &addr).await;
                break;
            }
        }
    }

    // If this section is reached it means that the client was disconnected!
    // Let's let everyone still connected know about it.
    {
        //notify server or client
        if is_server {
            let msg = format!("server [{}] has left the session", peer_id2);
            warn!("{}", msg);

            let msg_r2= "".to_string(); //send 0 bytes to client notify server is closed

            let mut state = state.lock().await;
            state.broadcast(&msg_r2 ).await;

            state.servers.remove(&addr);
        }
        else {
            notify_server_client_disconnected(&peer_id2, &state, &addr).await;
        }
    }

    Ok(())
}

async fn notify_server_client_disconnected( peer_id :&i64, state: &Arc<Mutex<Shared>>, addr: &SocketAddr) {
    let peer_id2 = peer_id.clone();

    let msg = format!("client [{}] has left the session", peer_id2);
    warn!("{}", msg);
    //state.broadcast(addr,  Bytes::from(msg)).await;

    let mut buf : Vec<u8> = [0u8; 12].to_vec();
    let client_id = peer_id.clone();
    buf[0]= ( client_id & 0xff ) as u8;
    buf[1] = ( ( client_id >> 8 ) & 0xff ) as u8;
    buf[2] = ( ( client_id >> 16 ) & 0xff ) as u8;
    buf[3] = ( ( client_id >> 24 ) & 0xff ) as u8;
    buf[4] = ( ( client_id >> 32 ) & 0xff ) as u8;
    buf[5] = ( ( client_id >> 40 ) & 0xff ) as u8;
    buf[6] = ( ( client_id >> 48 ) & 0xff ) as u8;
    buf[7] = ( ( client_id >> 56 ) & 0xff ) as u8;

    let msg_type = 20002;
    buf[8] = ( ( msg_type >> 0 ) & 0xff ) as u8;
    buf[9] = ( ( msg_type >> 8 ) & 0xff ) as u8;
    buf[10] = ( ( msg_type >> 16 ) & 0xff ) as u8;
    buf[11] = ( ( msg_type >> 24 ) & 0xff ) as u8;

    let mut msg_r = unsafe{ String::from_utf8_unchecked(buf) };

    let mut state = state.lock().await;
    state.sendto_server(&addr,  &msg_r ).await;

    warn!( "client [{}] {} disconnected, notify server", client_id, addr);

    state.peer_ids.remove(&client_id);
}