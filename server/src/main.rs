pub mod server_middleware;
pub mod server;

use server_middleware::DoS;
use tokio::sync::{mpsc, Mutex};
use tokio::task;
use std::env;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!("Usage: <program> <server_address> <load_request_port> <dos_port> [other_servers_election_port dos_port...]");
        return;
    }

    let server_address = args[1].clone();
    let load_request_address = args[2].parse().expect("Invalid load request port");
    let dos_port = args[3].parse().expect("Invalid DOS port");

    // Parse other servers into pairs of (election_port, dos_port)
    let mut other_servers = Vec::new();
    let mut i = 4;
    while i < args.len() - 1 {
        other_servers.push((args[i].clone(), args[i + 1].clone()));
        i += 2;
    }

    let (middleware_to_server_tx, middleware_to_server_rx) = mpsc::channel(32);
    let (server_to_middleware_tx, server_to_middleware_rx) = mpsc::channel(32);
    let server_to_middleware_rx = Arc::new(Mutex::new(server_to_middleware_rx));

    // Start the server middleware
    let server_middleware_handle = task::spawn(async move {
        server_middleware::run_server_middleware(
            server_address,
            load_request_address,
            dos_port,              // Fixed: removed type annotation
            middleware_to_server_tx,
            server_to_middleware_rx,
            other_servers,
        ).await;
    });

    // Start the server for encryption processing
    let server_handle = task::spawn(async move {
        server::run_server(middleware_to_server_rx, server_to_middleware_tx).await;
    });

    // Wait for both tasks to complete
    let _ = server_middleware_handle.await;
    let _ = server_handle.await;
}
