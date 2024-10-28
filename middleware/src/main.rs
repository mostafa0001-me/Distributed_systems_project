use std::env;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, Shutdown};

fn main() {
    // Get server IP addresses from command line
    let args: Vec<String> = env::args().collect();
    let server_ip = &args[1]; // For now, we'll use the first IP as the active server

    let listener = TcpListener::bind("127.0.0.1:7878").expect("Could not bind middleware to port");

    for stream in listener.incoming() {
        let stream = stream.expect("Failed to establish connection with client");
        handle_client(stream, server_ip);
    }
}

// Middleware function to handle the client and server communication
fn handle_client(mut client_stream: TcpStream, server_ip: &str) {
    let mut buffer = Vec::new();
    
    // Read the image from the client
    println!("Here1");
    client_stream.read_to_end(&mut buffer).expect("Failed to read image from client");
    println!("Here2");
    // Forward the image to the encryption server
    let mut server_stream = TcpStream::connect(server_ip).expect("Failed to connect to server");
    server_stream.write_all(&buffer).expect("Failed to send image to server");
    server_stream.shutdown(Shutdown::Write).expect("Failed to shut down writing side of stream");

    // Receive the encrypted image from the server
    let mut encrypted_buffer = Vec::new();
    server_stream.read_to_end(&mut encrypted_buffer).expect("Failed to read encrypted image from server");

    // Send the encrypted image back to the client
    client_stream.write_all(&encrypted_buffer).expect("Failed to send encrypted image to client");
}

