mod server_middleware;
mod server;

use std::sync::mpsc;
use std::env;
use std::thread;

fn main() {
    // Read server address and load request port from command-line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: <program> <server_address> <load_request_port> [other_servers...]");
        return;
    }

    let server_address = args[1].clone();
    let load_request_port: u16 = args[2].parse().expect("Invalid load request port");
    let other_servers: Vec<String> = args.iter().skip(3).cloned().collect();

    let (middleware_to_server_tx, middleware_to_server_rx) = mpsc::channel();
    let (server_to_middleware_tx, server_to_middleware_rx) = mpsc::channel();

    // Start the server middleware
    let server_middleware_handle = thread::spawn(move || {
        server_middleware::run_server_middleware(server_address, load_request_port, middleware_to_server_tx, server_to_middleware_rx);
    });

    // Start the server for encryption processing
    let server_handle = thread::spawn(move || {
        server::run_server(middleware_to_server_rx, server_to_middleware_tx);
    });

    // Wait for both threads to complete
    server_middleware_handle.join().expect("Server middleware thread failed");
    server_handle.join().expect("Server thread failed");
}

