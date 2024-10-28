use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, Shutdown};

fn main() {
    let server_ip = "127.0.0.1:8080"; // Server address
    let listener = TcpListener::bind("127.0.0.1:7879").expect("Could not bind server middleware to port");

    for stream in listener.incoming() {
        let stream = stream.expect("Failed to establish connection with client middleware");
        handle_client(stream, server_ip);
    }
}

fn handle_client(mut client_stream: TcpStream, server_ip: &str) {
    let mut buffer = Vec::new();

    // Read the image from the client middleware
    client_stream.read_to_end(&mut buffer).expect("Failed to read image from client middleware");

    // Forward the image to the server for encryption
    let mut server_stream = TcpStream::connect(server_ip).expect("Failed to connect to server");
    server_stream.write_all(&buffer).expect("Failed to send image to server");

    // Shut down the writing side of the connection to the server
    server_stream.shutdown(Shutdown::Write).expect("Failed to shut down writing side of server stream");

    // Receive the encrypted image from the server
    let mut encrypted_buffer = Vec::new();
    server_stream.read_to_end(&mut encrypted_buffer).expect("Failed to read encrypted image from server");

    // Send the encrypted image back to the client middleware
    client_stream.write_all(&encrypted_buffer).expect("Failed to send encrypted image to client middleware");
}

