//! `muxmultitool-server health`: the container health check against a real socket.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use muxmultitool_server::health::{ProbeError, probe};
use muxmultitool_server::{Config, Server};

fn config(frontend_dir: &std::path::Path) -> Config {
	Config {
		address: IpAddr::V4(Ipv4Addr::LOCALHOST),
		port: 0,
		frontend_dir: frontend_dir.to_path_buf(),
		..Config::default()
	}
}

#[tokio::test]
async fn probe_passes_on_a_running_server() {
	let dir = tempfile::tempdir().unwrap();
	let server = Server::bind(&config(dir.path())).await.unwrap();
	let addr = server.local_addr().unwrap();
	let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
	let running = tokio::spawn(server.run(async {
		stopped.await.ok();
	}));

	probe(addr).await.unwrap();

	stop.send(()).unwrap();
	running.await.unwrap().unwrap();
}

#[tokio::test]
async fn probe_fails_when_nothing_listens() {
	// Bind and drop a listener to get a port that is free right now.
	let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
	let addr: SocketAddr = listener.local_addr().unwrap();
	drop(listener);

	assert!(matches!(probe(addr).await, Err(ProbeError::Io(_))));
}

#[tokio::test]
async fn probe_fails_on_a_server_without_the_rpc() {
	// A plain HTTP server that answers 404 to everything.
	let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
		.await
		.unwrap();
	let addr = listener.local_addr().unwrap();
	tokio::spawn(async move {
		let (mut socket, _) = listener.accept().await.unwrap();
		let mut buffer = [0u8; 1024];
		let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buffer).await;
		tokio::io::AsyncWriteExt::write_all(
			&mut socket,
			b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
		)
		.await
		.unwrap();
	});

	assert!(matches!(probe(addr).await, Err(ProbeError::Status(_))));
}
