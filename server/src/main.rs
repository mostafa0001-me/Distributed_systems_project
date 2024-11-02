pub mod server_middleware;
pub mod server;

use tokio::sync::{mpsc, Mutex};
use tokio::task;
use std::env;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // Read server address and load request port from command-line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: <program> <server_address> <load_request_port> [other_servers...]");
        return;
    }

    let server_address = args[1].clone();
    let load_request_address: String = args[2].parse().expect("Invalid load request port");
    let other_servers: Vec<String> = args.iter().skip(3).cloned().collect();

    let (middleware_to_server_tx, middleware_to_server_rx) = mpsc::channel(32);
    let (server_to_middleware_tx, server_to_middleware_rx) = mpsc::channel(32);

    // Wrapping the receiver with Arc and Mutex to match expected types in server_middleware
    let server_to_middleware_rx = Arc::new(Mutex::new(server_to_middleware_rx));

    // Start the server middleware
    let server_middleware_handle = task::spawn(async move {
        server_middleware::run_server_middleware(
        server_address,
        load_request_address,
        middleware_to_server_tx,
        server_to_middleware_rx,
        other_servers
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
