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
use std::net::IpAddr;
use std::time::Duration;
//use tokio::stream::{Stream, StreamExt};
use tokio_stream::{Stream, StreamExt};
use tokio::sync::{mpsc, Mutex};
use tokio_util::codec::{Framed};

mod my_length_delimited;
use my_length_delimited::{MyLengthDelimitedCodec, MyBuilder}; 

use futures::SinkExt;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::io;

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use bytes::{Bytes};
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use std::sync::atomic::{Ordering};

//use bytes::BytesMut;
//use tini::Ini;
//use ini::Ini;

//use std::intrinsics::size_of;
use log::{info, warn};
/*use log4rs::{
    append::{
        console::{ConsoleAppender, Target},
        file::FileAppender,
    },
    config::{Appender, Config, Root},
    encode::pattern::PatternEncoder,
    filter::threshold::ThresholdFilter,
};*/

use console::Term;
use std::thread::sleep;
use std::sync::atomic::AtomicI16;
use socket2::{Socket, TcpKeepalive};
//use futures::core_reexport::sync::atomic::AtomicI64;
//use futures::core_reexport::cmp::Ordering;

/*#[macro_use]
extern crate log;
extern crate simple_logger;
extern crate simplelog;
*/

//use simplelog::*;
//use std::fs::File;

/*
enum MsgServer
{
    ID_CLIENT_CONNECTED = 20001,
    ID_CLIENT_DISCONNECTED = 20002,
    ID_CLIENT_DATA = 20003,
    ID_REGISTER_RESPONSE = 20004,
}

enum MsgClient
{
    ID_DATA_EVENT = 100001,
    ID_SERVER_DISCONNECTED = 100004,
}
*/
fn encode_head(src : &mut Vec<u8> ) -> Vec<u8> {
    src[0] = ( src[0] ^ 0xcf ) & 0xff;
    src[1] = ( src[1] ^ 0xcf ) & 0xff;
    src.to_vec()
}

static CLIENT_NUM: AtomicI16 = AtomicI16::new(0);

// Convert a `tokio::net::TcpStream` into a std stream, set TCP keepalive via
// socket2, then convert back to a tokio TcpStream. If `duration` is `None`,
// keepalive is disabled.
fn set_keepalive_socket(
    stream: tokio::net::TcpStream,
    duration: Option<Duration>,
) -> std::io::Result<tokio::net::TcpStream> {
    // Consume tokio stream to get std stream
    let std_stream = stream.into_std()?;
    let sock = Socket::from(std_stream);

    match duration {
        Some(dur) => {
            // Try to use TcpKeepalive builder (works on modern socket2)
            let ka = TcpKeepalive::new().with_time(dur).with_interval(Duration::from_secs(10));
            if let Err(e) = sock.set_tcp_keepalive(&ka) {
                warn!("set_tcp_keepalive failed: {}", e);
            }
        }
        None => {
            if let Err(e) = sock.set_keepalive(false) { warn!("set_keepalive(false) failed: {}", e); }
        }
    }

    let std_stream: std::net::TcpStream = sock.into();
    std_stream.set_nonblocking(true)?;
    let tok = tokio::net::TcpStream::from_std(std_stream)?;
    Ok(tok)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    // CombinedLogger::init(
    //     vec![
    //         TermLogger::new(LevelFilter::Warn,
    //                         ConfigBuilder::new().set_time_format_str("%Y-%m-%d %H:%M:%S").build(),
    //                         TerminalMode::Mixed).unwrap(),
    //         WriteLogger::new(LevelFilter::Info,
    //                          ConfigBuilder::new().set_time_format_str("%Y-%m-%d %H:%M:%S").build(),
    //                          File::create("net.log").unwrap()),
    //     ]
    // ).unwrap();

    println!("Current dir: {:?}", std::env::current_dir());


    // Initialize logger (log4rs not enabled in Cargo.toml for this workspace)
    //log4rs::init_file("config/log4rs.yaml", Default::default())
    match log4rs::init_file("config/log4rs.yaml", Default::default()) {     
        Ok(_) => println!("日志系统初始化成功: {}", "config/log4rs.yaml"),
        Err(e) => {
            eprintln!("日志初始化失败: {}", e);           
        }
    }
    info!("started");

    //info!("booting up");
    //error!("Bright red error\n");
    info!("gate started");
    //info!("gate started");
    //debug!("This level is currently not enabled for any logger\n");
    //warn!("This is an example message.");

    //simple_logger::init().unwrap();

    // Create the shared state. This is how all the peers communicate.
    //
    // The server task will hold a handle to this. For every new client, the
    // `state` handle is cloned and passed into the task that processes the
    // client connection.
    let state = Arc::new(Mutex::new(Shared::new() ) );

    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8080".to_string() );

    let gs_addr = env::args()
        .nth(2)
        .unwrap_or_else( || "127.0.0.1:8081".to_string() );

    let tcp_no_delay = env::args()
        .nth(3).unwrap_or_else( || "--tcp=delay".to_string() );
    let tcp_no_delay= if tcp_no_delay.ends_with("nodelay") { true } else { false };

    info!("tcp_no_delay: {}", tcp_no_delay );

    // Bind a TCP listener to the socket address.
    //
    // Note that this is the Tokio TcpListener, which is fully async.
    let  listener = TcpListener::bind(&addr).await?;
    let  gs_listener = TcpListener::bind(&gs_addr).await?;

    info!("gate running on client:{}, gs:{}", addr, gs_addr);

    let term = Term::stdout();
    // Save an original title (friendly program name) and append client count to it
    let original_title = env::args()
        .nth(0)
        .and_then(|p| std::path::Path::new(&p).file_name().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "gate_server".to_string());
    //term.set_title()

    let gs_ip : Vec<&str> = addr.split(':').collect();
    let gs_ip = gs_ip[0];
    let gs_local_addr = IpAddr::from_str(&gs_ip).unwrap();

    // Spawn a task to accept multiple GS connections concurrently.
    {
        let gs_listener = gs_listener;
        let gs_state = Arc::clone(&state);
        let gs_local_addr = gs_local_addr;
        tokio::spawn(async move {
            let mut gs_peer_id: i64 = 0;
            loop {
                match gs_listener.accept().await {
                    Ok((mut gs_stream, gs_addr)) => {
                        info!("server is incoming {}", gs_addr);

                        // Read initial 4 bytes from server stream before configuring socket
                        let mut gs_header = [0u8; 4];
                        if let Err(e) = gs_stream.read_exact(&mut gs_header).await {
                            warn!("failed to read initial 4 bytes from gs_stream: {}", e);
                            let _ = gs_stream.shutdown().await;
                            continue;
                        }

                        // Compare against expected header (big-endian). If mismatch, close connection.
                        let expected: u32 = 0x1357_9753;
                        let got = u32::from_le_bytes(gs_header);
                        if got != expected {
                            warn!("gs initial header mismatch: got=0x{:08x}, expected=0x{:08x}; closing", got, expected);
                            let _ = gs_stream.shutdown().await;
                            continue;
                        }

                        // Configure keepalive and nodelay
                        let  gs_stream = match set_keepalive_socket(gs_stream, Some(Duration::from_secs(60))) {
                            Ok(s) => s,
                            Err(e) => {
                                warn!("set keepalive failed for gs_stream: {}", e);
                                continue;
                            }
                        };

                        if let Err(e) = gs_stream.set_nodelay(true) { warn!("set_nodelay failed: {}", e); }

                        gs_peer_id += 1;
                        let peer_id = gs_peer_id;
                        let gs_state2 = Arc::clone(&gs_state);

                        tokio::spawn(async move {
                            if let Err(e) = process(gs_state2, gs_stream, gs_local_addr, gs_addr, peer_id, true).await {
                                warn!("an error occurred; GS connection {} {} error = {:?}", peer_id, gs_addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        warn!("gs accept failed: {}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });
    }

    tokio::spawn(async move {
        loop {
            sleep(Duration::new(1, 0));
            let n = CLIENT_NUM.load(Ordering::SeqCst);
            let title = format!("{} - clients：{}", original_title, n);
            let _ = term.set_title(&title);
        }
    });

    let ip : Vec<&str> = addr.split(':').collect();
    let ip = ip[0];
    let local_addr = IpAddr::from_str(&ip).unwrap();

    let mut peer_id = 0i64;
    loop {
        // Asynchronously wait for an inbound TcpStream.
        let (stream, _addr) = listener.accept().await?;

        // Set keepalive using socket2 and then set nodelay.
        let mut stream = match set_keepalive_socket(stream, Some(Duration::from_secs(60*10))) {
            Ok(s) => s,
            Err(e) => {
                warn!("set keepalive failed for client stream: {}", e);
                continue;
            }
        };

        let tcp_no_delay = tcp_no_delay.clone();
        stream.set_nodelay(tcp_no_delay)?;

        // Clone a handle to the `Shared` state for the new connection.
        let state = Arc::clone(&state);

        peer_id += 1;
        CLIENT_NUM.fetch_add(1, Ordering::SeqCst);


        info!("client [{}] {} has connected", peer_id, _addr);

        // Spawn our handler to be run asynchronously.
        tokio::spawn(async move {
            // Check if the stream is readable (has data) within 10 seconds.
            // If no data arrives, disconnect this client.
            match tokio::time::timeout(Duration::from_secs(10), stream.readable()).await {
                Ok(Ok(())) => {
                    // stream is readable, proceed
                }
                Ok(Err(e)) => {
                    warn!("stream readable check failed for [{}] {}: {}", peer_id, _addr, e);
                    let _ = stream.shutdown().await;
                    CLIENT_NUM.fetch_sub(1, Ordering::SeqCst);
                    return;
                }
                Err(_) => {
                    // timeout: no message received within 10s
                    warn!("connection [{}] {} timed out (no message within 10s), disconnecting", peer_id, _addr);
                    let _ = stream.shutdown().await;
                    CLIENT_NUM.fetch_sub(1, Ordering::SeqCst);
                    return;
                }
            }

            //client incoming
            {
                let ip = _addr.ip().to_string();
                let port = _addr.port().to_string();

                let msg_type:i32 = 20001;
                let mut msg_r = peer_id.to_le_bytes().to_vec();    //unsafe { String::from_utf8_unchecked(peer_id2.to_le_bytes().to_vec() ) };
                let msg_type = msg_type.to_le_bytes().to_vec(); //unsafe { String::from_utf8_unchecked( msg_type.to_le_bytes().to_vec() ) };
                //msg_r.push_str( &msg_type );
                //msg_r.push_str(&ip);
                //msg_r.push(':');
                //msg_r.push_str(&port);
                msg_r.extend(msg_type);
                msg_r.extend(Bytes::from(ip ) );
                msg_r.extend(Bytes::from(":" ) );
                msg_r.extend(Bytes::from(port ) );

                let mut state0 = state.lock().await;
                state0.sendto_server(Bytes::from(msg_r)).await;
            }

            if let Err(e) = process(state, stream, local_addr, _addr, peer_id, false).await {
                warn!("an error occurred; 000 !!!! connection {} {} error = {:?}", peer_id, _addr, e);
            }

            CLIENT_NUM.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

/// Shorthand for the transmit half of the message channel.
type Tx = mpsc::UnboundedSender<Bytes>;

/// Shorthand for the receive half of the message channel.
type Rx = mpsc::UnboundedReceiver<Bytes>;

/// Data that is shared between all peers in the chat server.
///
/// This is the set of `Tx` handles for all connected clients. Whenever a
/// message is received from a client, it is broadcasted to all peers by
/// iterating over the `peers` entries and sending a copy of the message on each
/// `Tx`.
struct Shared {
    //peers: HashMap<SocketAddr, Tx>,
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
    frames: Framed<TcpStream,  MyLengthDelimitedCodec>,

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
            //peers: HashMap::new(),
            peer_ids : HashMap:: new(),
            servers : HashMap::new(),
        }
    }

    /// Send a message to every clients
    async fn broadcast(&mut self, message: Bytes) {
        for peer in self.peer_ids.iter_mut() {
            //if *peer.0 == id
            {
                let _ = peer.1.send( message.clone());
            }
        }
    }

    async fn sendto_server(&mut self, message: Bytes) {
        for server in self.servers.iter_mut() {
            //if *server.0 == server
            {
                //let mut msg : Bytes = Bytes::new();
                //msg.clone_from(message)
                let _ = server.1.send( message.clone() );
            }
        }
        // if let Some(server) = self.servers.get_key_value(&server) {
        //     let _ = server.1.send( message );
        // }
    }
    async fn sendto_client_by_id(&mut self, id: i64, message: Bytes) {
        // for peer in self.peer_ids.iter_mut() {
        //     if *peer.0 == id {
        //         let _ = peer.1.send(message);
        //     }
        // }
        if let Some(peer) = self.peer_ids.get_key_value(&id) {
            let _ = peer.1.send( message);
        }
    }
}

impl Peer {
    /// Create a new instance of `Peer`.
    async fn new(
        state: Arc<Mutex<Shared>>,
        frames: Framed<TcpStream, MyLengthDelimitedCodec>,
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
    //Broadcast(String),

    FromServer(Bytes ),
    FromClient(Bytes),

    /// A message that should be received by a client
    Received(Bytes),
    //ErrorOccurred(Bytes),
    ErrorOccurred(String),
}

// Peer implements `Stream` in a way that polls both the `Rx`, and `Framed` types.
// A message is produced whenever an event is ready until the `Framed` stream returns `None`.
impl Stream for Peer {
    type Item = Result<Message, ()>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // First poll the `UnboundedReceiver`.

        if let Poll::Ready(Some(v)) = Pin::new(&mut self.rx).poll_recv(cx) {
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
                    Some(Ok(Message::FromServer( Bytes::from(message.to_vec()))))
                }
                else {
                    //给服务器的消息
                    Some(Ok(Message::FromClient( Bytes::from(message.to_vec()))))
                }
                // We've received a message we should broadcast to others.
                //Some(Ok(Message::Broadcast(message.to_vec())))
            },

            // An error occured.
            //Some(Err(e)) => Some(Err(e)),
            //Some(Err(e)) => Some(Err( e.source().unwrap())),
            //Some(Err(e)) => Some( Ok(Message::ErrorOccurred( Bytes::from(e.to_string() ) ) ) ),
            Some(Err(e)) => Some( Ok(Message::ErrorOccurred( e.to_string() ) ) ),
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
    _local_addr: IpAddr,
    addr: SocketAddr,
    peer_id : i64,
    is_server : bool,
) -> Result<(), Box<dyn Error>> {
    let mut builder: MyBuilder = MyBuilder::new();
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
    let io_packet = Framed::new(stream, MyLengthDelimitedCodec::new_from_builder(builder));
    //let codec = LengthDelimitedCodec::builder().little_endian();
    //let mut io_packet = Framed::new(stream, LengthDelimitedCodec::);

    //TODO!另开线程监听服务器端口，由于线程与task通信暂时麻烦，就从配置文件判断是否是服务器ip了

    //let server_name : String = "".to_string();
    let username = "";

    let is_server = is_server.clone();
    let is_server2 = is_server.clone();

    let peer_id2 : i64 = peer_id.clone();
    let peer_id0 = peer_id.clone();
    // Register our peer with state which internally sets up some channels.
    let mut peer = Peer::new(state.clone(), io_packet, is_server2, peer_id0 ).await?;

    //let first_packet_to_client = true; //first packet is the random seed, a int, no length field

    // Process incoming messages until our stream is exhausted by a disconnect.
    while let Some(result) = peer.next().await {
        match result {
            // A message was received from the current user, we should
            // broadcast this message to the other users.
            /*Ok(Message::Broadcast(msg)) => {
                //let mut state = state.lock().await;
                //let msg = format!("{}: {}", username, String::from_utf8(msg).unwrap());

                if is_server {
                    //state.broadcast(addr, &msg.into_bytes()).await;
                    //state.sendto_client_by_id(id,  &msg).await;
                }
                else {
                    //state.sendto_server(addr,  Bytes::from(msg)).await;
                }
            }*/

            Ok(Message::FromServer(msg)) => {
                let mut state = state.lock().await;

                assert_eq!(is_server, true);

                //get client_id from msg content
                if is_server {
                    //let msg_r = msg.clone();
                    let id = [msg[0], msg[1], msg[2], msg[3], msg[4], msg[5], msg[6], msg[7]];
                    let client_id0= i64::from_le_bytes( id );

                    //在encoder里不额外添加包头发给客户端
                    //let mut msg_r2 = "".to_string();
                    //msg_r2.push_str(unsafe { &String::from_utf8_unchecked(msg_r[8..].to_vec()) } );
                    state.sendto_client_by_id( client_id0, Bytes::from(msg[8..].to_vec()) ).await;
                }
            }
            Ok(Message::FromClient(msg)) => {
                let mut state = state.lock().await;

                assert_eq!(is_server, false);

                let client_id = peer_id;
                let msg_type : i32 = 20003;

                //let mut msg_r = unsafe { String::from_utf8_unchecked( client_id.to_le_bytes().to_vec() ) };
                //let msg_type = unsafe{ String::from_utf8_unchecked( msg_type.to_le_bytes().to_vec() ) };
                //msg_r.push_str( &msg_type );
                let mut msg_r = client_id.to_le_bytes().to_vec();
                let msg_type = msg_type.to_le_bytes().to_vec();
                msg_r.extend(msg_type );

                let len = msg.len()+2;  //+2 length field length decode时去除了包头（长度），因此这里要加上再给服务器
                let mut len1 = vec!( (len & 0xff) as u8, ( ( len >> 8 ) & 0xff) as u8 );
                let len2 = encode_head( &mut len1 ); //在length_delimiter里调用了一次是解密，再次则是加密

                //let len3 = unsafe { String::from_utf8_unchecked(len2) } ; //如果不是有效utf8，则会分配并替换为有效的utf8字符
                //msg_r.push_str( &len3 );
                //msg_r.push_str(&msg[..]);
                msg_r.extend(len2);
                msg_r.extend(&msg[..] );

                state.sendto_server( Bytes::from( msg_r.to_vec() ) ).await;
            }

            // A message was received from a peer. Send it to the current user.
            Ok(Message::Received(msg)) => {
                //println!("recv is_server {},peer.is_server {} ", &is_server, &peer.is_server);
                if msg.len() == 0 {
                    if !is_server {
                        warn!("close the client [{}] {} because the server is disconnected", peer_id, addr);
                    }
                    if let Err(e) = peer.frames.into_inner().shutdown().await {
                        warn!("shutdown {} {} failed with {} ", peer_id, addr, e);
                    }
                    break;
                }
                if !is_server { //msg sent to client
                    if msg.len() > 4 {
                        let inner_msg_type = msg[4..5].to_vec();  //unsafe { msg.get_unchecked(4..5) };
                        //let inner_msg_type= inner_msg_type.to_vec();//String::from(inner_msg_type ).into_bytes();
                        let inner_msg_type =  ( inner_msg_type[0] ^ 0xcf ) & 0xff;
                        if inner_msg_type == 3 || inner_msg_type == 14 {
                            warn!("connection [{}] {} closed, because recv MSG_CLOSE or MSG_DISCONNECT", peer_id, addr);

                            let mut state = state.lock().await;
                            let client_id = peer_id;
                            state.peer_ids.remove(&client_id);

                            if let Err(e) = peer.frames.close().await{
                                warn!("close {} {} failed with {} ", peer_id, addr, e);
                            }
                            if let Err(e) = peer.frames.into_inner().shutdown().await {
                                warn!("shutdown {} {} failed with {} ", peer_id, addr, e);
                            }
                            return Ok(());
                        }
                    }
                }

                if let Err(e) = peer.frames.send(Bytes::from(msg)).await { //send msg to client or server
                    warn!("connection {} {} closed send failed ", peer_id, addr);
                    warn!("an error occurred; connection {} {} error = {:?}", peer_id, addr, e);
                    /*
                    let mut state = state.lock().await;
                    let client_id = peer_id.clone();
                    state.peer_ids.remove(&client_id);
                    if let Err(e) = peer.frames.close().await{
                        warn!("close {} {} failed with {} ", peer_id, addr, e);
                    }
                    if let Err(e) = peer.frames.into_inner().shutdown().await {
                        warn!("shutdown {} {} failed with {} ", peer_id, addr, e);
                    }
                    */
                    //返回之前先通知服务器客户端断开了
                    if !is_server {
                        notify_server_client_disconnected( &peer_id, &state, &addr).await;
                    }
                    if let Err(e) = peer.frames.close().await{
                        warn!("close {} {} failed with {} ", peer_id, addr, e);
                    }
                    if let Err(e) = peer.frames.into_inner().shutdown().await {
                        warn!("shutdown {} {} failed with {} ", peer_id, addr, e);
                    }
                    return Err(Box::<dyn Error>::from(e));
                };
            }
            Ok(Message::ErrorOccurred(e)) => {
                println!( "{}", e );
                break;
            }

            Err(e) => {
                warn!( "an error occurred while processing messages for {}; error = {:?}",username, e);

                //notify_server_client_disconnected(&peer_id, &state, &addr).await;
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

            //let msg_r= "".to_string(); //send 0 bytes to client notify server is closed
            let msg_r = Bytes::from("");

            let mut state = state.lock().await;
            state.broadcast(msg_r ).await;

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

    let client_id = peer_id;//.clone();
    let msg_type : i32 = 20002;

    let mut msg_r = client_id.to_le_bytes().to_vec();    //unsafe{ String::from_utf8_unchecked(client_id.to_le_bytes().to_vec()) };
    let msg_type = msg_type.to_le_bytes().to_vec();  //unsafe{ String::from_utf8_unchecked( msg_type.to_le_bytes().to_vec()) };
    //msg_r.push_str( &msg_type );
    msg_r.extend(msg_type );

    let mut state = state.lock().await;
    state.sendto_server(Bytes::from(msg_r) ).await;

    warn!( "client [{}] {} disconnected, notify server", client_id, addr);

    state.peer_ids.remove(&client_id);
}