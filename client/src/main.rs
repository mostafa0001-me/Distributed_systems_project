pub mod client;
pub mod middleware;

use tokio::sync::mpsc;
use std::env;
use client::ImageRequest;

#[tokio::main]
async fn main() {
    // Collect server IPs from command line arguments
    let client_ip : String = env::args().nth(1).expect("Please provide the client IP as a command-line argument.");
    let server_ips: Vec<String> = env::args().skip(1).collect();

    if server_ips.is_empty() {
        eprintln!("Please provide at least one server IP as a command-line argument.");
        return;
    }

    // Create channels for communication between client and middleware
    let (client_to_middleware_tx, client_to_middleware_rx) = mpsc::channel::<ImageRequest>(32); // Buffer size of 32
    let (middleware_to_client_tx, middleware_to_client_rx) = mpsc::channel(32);
//:Sender<Vec
    // Spawn the middleware task
    let middleware_handle = tokio::spawn(async move {
        middleware::run_middleware(
            client_to_middleware_rx,
            middleware_to_client_tx,
            server_ips
        ).await
    });

    // Spawn the client task
    let client_handle = tokio::spawn(async move {
        client::run_client(
            client_to_middleware_tx,
            middleware_to_client_rx, 
            client_ip,
            10
        ).await
    });

    // Wait for both tasks to finish
    let (middleware_result, client_result) = 
        tokio::join!(middleware_handle, client_handle);

    // Handle any errors
    if let Err(e) = middleware_result {
        eprintln!("Middleware task failed: {}", e);
    }
    if let Err(e) = client_result {
        eprintln!("Client task failed: {}", e);
    }
}
