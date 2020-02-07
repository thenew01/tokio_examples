//! A "hello world" echo server with Tokio
//!
//! This server will create a TCP listener, accept connections in a loop, and
//! write back everything that's read off of each TCP connection.
//!
//! Because the Tokio runtime uses a thread pool, each TCP connection is
//! processed concurrently with all other TCP connections across multiple
//! threads.
//!
//! To see this server in action, you can run this in one terminal:
//!
//!     cargo run --example echo
//!
//! and in another terminal you can run:
//!
//!     cargo run --example connect 127.0.0.1:8080
//!
//! Each line you type in to the `connect` terminal should be echo'd back to
//! you! If you open up multiple terminals running the `connect` example you
//! should be able to see them all make progress simultaneously.

#![warn(rust_2018_idioms)]

use tokio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::stream::{Stream, StreamExt};
use tokio::sync::{mpsc, Mutex};
use tokio_util::codec::{Framed, LinesCodec};

use std::env;
use std::error::Error;
use std::str::FromStr;
use std::thread;
use std::time::Duration;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::sync::atomic::*;

use std::io;

use std::collections::HashMap;

use std::net::SocketAddr;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ini::Ini;
use copy_in_place::copy_in_place;

type Tx = mpsc::UnboundedSender<Vec<u8>>;
type Rx = mpsc::UnboundedReceiver<Vec<u8>>;

struct Shared {
	peers: HashMap<SocketAddr, Tx>,
	peer_ids : HashMap<i32, Tx>,
	servers : HashMap<SocketAddr, Tx>,
	server_names : HashMap<String, Tx>,
}

struct Peers {
	rx : Rx,
	is_server : bool,
}

impl Shared {
	/// Create a new, empty, instance of `Shared`.
	fn new() -> Self {
		Shared {
			peers: HashMap::new(),
			peer_ids : HashMap:: new(),
			servers : HashMap::new(),
			server_names :  HashMap::new(),
		}
	}

	/// Send a `LineCodec` encoded message to every peer, except
	/// for the sender.
	async fn sendto_client(&mut self, client: SocketAddr, message: &Vec<u8>) {
		for peer in self.peers.iter_mut() {
			//if *peer.0 == client
			{
				let _ = peer.1.send(message.to_vec());
			}
		}
	}
	async fn sendto_server(&mut self, server: SocketAddr, message: &Vec<u8>) {
		for server in self.servers.iter_mut() {
			//if *server.0 == server
			{
				let _ = server.1.send(message.to_vec());
			}
		}
	}
	async fn sendto_client_by_id(&mut self, id: i32, message: &Vec<u8>) {
		for peer in self.peer_ids.iter_mut() {
			if *peer.0 == id {
				let _ = peer.1.send(message.to_vec());
			}
		}
	}
}

impl Peers {
	async fn new(
		state: Arc<Mutex<Shared>>,
		client_id: i32,
		is_server : bool,
	) -> io::Result<Peers> {
		// Create a channel for this peer
		let (tx, rx) = mpsc::unbounded_channel();
		state.lock().await.peer_ids.insert(client_id, tx);
		let is_server = is_server;
		Ok(Peers { rx, is_server })
	}

	async fn new2(
		state: Arc<Mutex<Shared>>,
		client_id: i32,
		is_server : bool,
		server_name : String,
	) -> io::Result<Peers> {
		// Create a channel for this peer
		let (tx, rx) = mpsc::unbounded_channel();
		state.lock().await.server_names.insert(client_id, tx);
		let is_server = is_server;
		Ok(Peers { rx, is_server })
	}

}
impl Stream for Peers{
	type Item = Result<Vec<u8>, ()>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		// First poll the `UnboundedReceiver`.
		if let Poll::Ready(Some(v)) = Pin::new(&mut self.rx).poll_next(cx) {
			return Poll::Ready(Some(Ok(v)));
		}
		else {
			return Poll::Ready(Some(Ok(Vec::new())));
		}
	}

}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

	let addr = env::args()
		.nth(1)
		.unwrap_or_else(|| "127.0.0.1:8080".to_string());

	let mut listener = TcpListener::bind(&addr).await?;
	println!("Listening on: {}", addr);

	let clients = Arc::new(AtomicUsize::new(0));
	let counter1 = Arc::clone(&clients);

	let state = Arc::new(Mutex::new(Shared::new()));


	let ip : Vec<&str> = addr.split(':').collect();
	let ip = ip[0];
	let local_addr = IpAddr::from_str(&ip).unwrap();

	thread::spawn( move ||{
		loop{
			thread::sleep(Duration::from_secs(1));
			println!("clients: {}", counter1.load(Ordering::Relaxed));
		}
	});

	let mut peer_id = 0;
	loop {
		let (mut socket, _addr) = listener.accept().await?;

		let counter2 = Arc::clone(&clients);
		counter2.fetch_add(1, Ordering::SeqCst);

		peer_id += 1;
		let peer_id2 = peer_id.clone();
		let peer_id3 = peer_id.clone();

		let state = Arc::clone(&state);

		tokio::spawn(async move {
			//let mut lines = Framed::new(socket, LinesCodec::new());
			let mut is_server = false;
			{
				let i = Ini::load_from_file("./center_server_packet.ini").unwrap();
				for (sec, prop) in i.iter() {
					println!("Section: {:?}", *sec);
					for (k, v) in prop.iter() {
						println!("{}:{}", *k, *v);
						if *v == local_addr.to_string() {
							is_server = true;
							break;
						}
					}
				}

				if is_server == false {
//					let mut buf : Vec<u8> = [0u8; 64].to_vec();
//					buf[0]= ( peer_id2 & 0xff ) as u8;
//					buf[1] = ( ( peer_id2 >> 8 ) & 0xff ) as u8;
//					buf[2] = ( ( peer_id2 >> 16 ) & 0xff ) as u8;
//					buf[3] = ( ( peer_id2 >> 24 ) & 0xff ) as u8;
//
//					let mut state = state.lock().await;
//					state.sendto_server(_addr, &buf).await;
				}
			}
			if let Err(e) = process(state, socket, _addr, peer_id3, is_server, counter2).await {
				println!("an error occured; error = {:?}", e);
			}
		});
	}
}

/// Process an individual chat client
async fn process(
	state: Arc<Mutex<Shared>>,
	mut socket: TcpStream,
	//local_addr: IpAddr,
	_addr: SocketAddr,
	peer_id3 : i32,
	is_server : bool,
	mut counter2 : Arc<AtomicUsize>,
) -> Result<(), Box<dyn Error>> {

	//tokio::net::tcp::TcpStream没办法放在hashmap里面
	let mut peer = Peers::new(state.clone(), peer_id3, is_server).await?;

	let first_packet = true;
	// In a loop, read data from the socket and write the data back.\
	loop { //TODO! Stream.next vs loop
		if is_server == false {
			//while let Some(result) = peer.next().await {
			if let Some(result) = peer.next().await {
				match result {
					Ok(msg) => { //msg from server
						//println!("recv is_server {},peer.is_server {} ", &is_server, &peer.is_server);
						//peer.lines.send( Bytes::from(msg) ).await?;
					}
					Err(e) => {
						println!("an error occured while processing messages for ; error = {:?}", e);
					}
				}
			}
		}

		//let mut socket = socket.clone();
		let mut buf = [0; 1024];
		let mut i = 0;
		let n = socket
			.read(&mut buf[i..1024])
			.await
			.unwrap_or_else( |err| {
				println!("failed to read data from socket， {}", err);
				100000
			});

		if n == 100000 {
			println!("recv flag 100000, client {} disconnected", _addr);
			counter2.fetch_sub(1, Ordering::SeqCst);
			return Ok(());
		}

		if n == 0 {
			println!("recv 0 bytes, client {} disconnected", _addr);
			counter2.fetch_sub(1, Ordering::SeqCst);
			return Ok(());
		}

		i += n;

		if i > 2 { //大于包头长度
			let total = i.clone();
			let mut len : usize = 0;
			let mut offset : usize = 0;
			let mut j : usize = 0;

			loop {
				len = buf[j] as usize;
				len |= (buf[j + 1] as usize) << 8;
				if i >= len as usize { //够一个包
//
//					if first_packet {
//
//					}

					if is_server { //send to client


					}
					else { // send to server

					}
					offset += &len;
					socket
						.write_all(&buf[j..offset])
						.await
						.expect("failed to write data to socket");

					i -= &len;
					j += &len;
					if  i == 0 {
						break;
					}
					if i > 0 && i <= 2 { //剩余只够一个包头
						copy_in_place(&mut buf, j..total, 0); //把剩余的移到buf前头
						break;
					}
				}
				else {
					if j != 0 && j < total { //至少收到过包
						copy_in_place(&mut buf, j..total, 0); //把剩余的移到buf前头
					}
					break;
				}
			}
		}
	}

	Ok(())
}