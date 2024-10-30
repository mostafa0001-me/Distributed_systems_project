use std::sync::mpsc::{Receiver, Sender};
use std::net::TcpStream;
use std::io::{Read, Write};
use std::time::Duration;
use std::thread;

// Main function to handle image data transmission through an elected server
pub fn run_middleware(rx: Receiver<Vec<u8>>, tx: Sender<Vec<u8>>, server_ips: Vec<String>) {
    // Attempt to connect and select an available server
    if let Some(mut server_stream) = find_available_server(&server_ips) {
        println!("Middleware: Maintaining connection with the selected server.");

        // Receive image data from client
        let image_data = rx.recv().expect("Failed to receive image data from client");
        println!("Middleware: Image data received from client.");

        // Send image data to the selected server
        server_stream.write_all(&image_data).expect("Failed to send image data to server");

        // Signal to the server that all data has been sent and it can start processing
        server_stream.shutdown(std::net::Shutdown::Write).expect("Failed to shutdown write stream");

        // Receive encrypted response from the server
        let mut response = Vec::new();
        server_stream.read_to_end(&mut response).expect("Failed to read encrypted response from server");

        // Send the encrypted response back to the client
        tx.send(response).expect("Failed to send response to client");
    } else {
        eprintln!("Middleware: No servers are available to handle the request.");
    }
}

// Function to find and maintain an active connection with the selected server
fn find_available_server(server_ips: &[String]) -> Option<TcpStream> {
    for server_ip in server_ips {
        // Attempt to connect to each server and send the "I want to send" message
        if let Ok(mut stream) = TcpStream::connect(server_ip) {
            // Send the lightweight message
            if stream.write_all(b"I want to send").is_ok() {
                println!("Middleware: Sent 'I want to send' message to server at {}.", server_ip);

                // Wait for the server's response confirming it has accepted the request
                let mut buffer = [0; 1024];
                if let Ok(bytes_read) = stream.read(&mut buffer) {
                    let response = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();
                    if !response.is_empty() {
                        println!("Middleware: Server at {} accepted the request.", response.trim());
                        // Return the active stream with the selected server
                        return Some(stream);
                    }
                }
            }
        }
        // Add a brief delay to avoid overwhelming the servers
        thread::sleep(Duration::from_millis(50));
    }
    None
}

