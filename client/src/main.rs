mod client;
mod middleware;

use std::sync::mpsc;
use std::thread;
use std::env;

fn main() {
    // Collect server IPs from command line arguments
    let server_ips: Vec<String> = env::args().skip(1).collect();

    if server_ips.is_empty() {
        eprintln!("Please provide at least one server IP as a command-line argument.");
        return;
    }

    // Create channels for communication between client and middleware
    let (client_to_middleware_tx, client_to_middleware_rx) = mpsc::channel();
    let (middleware_to_client_tx, middleware_to_client_rx) = mpsc::channel();

    // Start the middleware thread with server IPs
    let middleware_handle = thread::spawn(move || {
        middleware::run_middleware(client_to_middleware_rx, middleware_to_client_tx, server_ips);
    });

    // Start the client thread with both channels
    let client_handle = thread::spawn(move || {
        client::run_client(client_to_middleware_tx, middleware_to_client_rx);
    });

    // Wait for both threads to finish
    middleware_handle.join().expect("Middleware thread failed");
    client_handle.join().expect("Client thread failed");
}

