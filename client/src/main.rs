mod client;
mod middleware;

use tokio::sync::mpsc;
use std::env;
use client::{ImageRequest, ImageResponse};

#[tokio::main]
async fn main() {
    // Collect server IPs from command line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <client_ip> <server_ip1> [<server_ip2> ...]", args[0]);
        return;
    }

    let client_ip = args[1].clone();
    let client_ip2 = args[1].clone();
    let server_ips: Vec<String> = args[2..].to_vec();

    if server_ips.is_empty() {
        eprintln!("Please provide at least one server IP as a command-line argument.");
        return;
    }

    // Create channels for communication between client and middleware
    let (client_to_middleware_tx, client_to_middleware_rx) = mpsc::channel::<ImageRequest>(32);
    let (middleware_to_client_tx, middleware_to_client_rx) = mpsc::channel::<ImageResponse>(32);

    // Spawn the middleware task
    let middleware_handle = tokio::spawn(async move {
        middleware::run_middleware(
            client_to_middleware_rx,
            middleware_to_client_tx,
            server_ips
        ).await
    });

    // Spawn the client server
    let client_server_handle = tokio::spawn(async move {
        client::run_client_server(client_ip).await;
    });

    // Spawn the client task
    let client_handle = tokio::spawn(async move {
        client::run_client(
            client_to_middleware_tx,
            middleware_to_client_rx, 
            client_ip2,
            1
        ).await
    });

    // Wait for all tasks to finish
    let (middleware_result, client_server_result, client_result) = 
        tokio::join!(middleware_handle, client_server_handle, client_handle);

    // Handle any errors
    if let Err(e) = middleware_result {
        eprintln!("Middleware task failed: {}", e);
    }
    if let Err(e) = client_server_result {
        eprintln!("Client server task failed: {}", e);
    }
    if let Err(e) = client_result {
        eprintln!("Client task failed: {}", e);
    }
}

