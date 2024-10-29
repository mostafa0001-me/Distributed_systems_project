use std::sync::mpsc::{Receiver, Sender};
use std::net::TcpStream;
use std::io::{Read, Write};
use std::thread;

pub fn run_middleware(rx: Receiver<Vec<u8>>, tx: Sender<Vec<u8>>, server_ips: Vec<String>) {
    // Receive image data from client
    let image_data = rx.recv().expect("Failed to receive image data from client");
    println!("Middleware: Image data received from client.");

    // Create a vector to hold all server threads
    let mut server_threads = vec![];

    // Spawn a thread for each server to send the image data concurrently
    for server_ip in server_ips {
        let data = image_data.clone(); // Clone the data for each thread
        let tx_clone = tx.clone(); // Clone the transmitter to send responses back to client

        let handle = thread::spawn(move || {
            match connect_and_send(&server_ip, &data) {
                Ok(response) => {
                    println!("Middleware: Received response from server at {}.", server_ip);
                    tx_clone.send(response).expect("Failed to send response to client");
                }
                Err(e) => {
                    eprintln!("Middleware: Failed to communicate with server {}: {}", server_ip, e);
                }
            }
        });
        server_threads.push(handle);
    }

    // Wait for all server threads to complete
    for handle in server_threads {
        handle.join().expect("Failed to join server thread");
    }
}

// Function to connect to a server, send image data, and receive response
fn connect_and_send(server_ip: &str, image_data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut server_stream = TcpStream::connect(server_ip)?;

    // Send the image data to the server
    server_stream.write_all(image_data)?;
    server_stream.shutdown(std::net::Shutdown::Write)?;

    // Receive the encrypted response from the server
    let mut response = Vec::new();
    server_stream.read_to_end(&mut response)?;
    Ok(response)
}

