use std::env;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc::Sender, Arc, Mutex};
use std::sync::mpsc::Receiver;
use std::thread;

// A structure to hold the server's state, including its load.
struct ServerState {
    load: u32,
}

impl ServerState {
    fn new() -> Self {
        ServerState { load: 0 }
    }

    fn increment_load(&mut self) {
        self.load += 1;
    }

    fn decrement_load(&mut self) {
        if self.load > 0 {
            self.load -= 1;
        }
    }

    fn current_load(&self) -> u32 {
        self.load
    }
}

pub fn run_server_middleware(server_address: String, load_request_port: u16, server_tx: Sender<Vec<u8>>, server_rx: Receiver<Vec<u8>>) {
    // Parse command-line arguments to obtain other server addresses
    let args: Vec<String> = env::args().collect();
    let other_servers: Vec<String> = args.iter().skip(3).cloned().collect();

    let listener = TcpListener::bind(&server_address).expect("Could not bind to server address");

    // Wrap `server_rx` in Arc and Mutex to make it thread-safe and shareable
    let server_rx = Arc::new(Mutex::new(server_rx));
    let state = Arc::new(Mutex::new(ServerState::new()));

    // Start a separate thread to listen for load requests from other servers on a unique port
    let load_listener_state = Arc::clone(&state);
    let load_listener_address = format!("0.0.0.0:{}", load_request_port);
    thread::spawn(move || listen_for_load_requests(load_listener_address, load_listener_state));

    for stream in listener.incoming() {
        let stream = stream.expect("Failed to accept connection");
        let server_tx = server_tx.clone();
        let server_rx = Arc::clone(&server_rx);
        let state = Arc::clone(&state);
        let other_servers = other_servers.clone();
        let server_address = server_address.clone(); // Clone `server_address` for each thread

        // Spawn a thread to handle each connection with the main middleware
        thread::spawn(move || {
            handle_connection(stream, server_tx, server_rx, state, other_servers, server_address);
        });
    }
}


fn handle_connection(
    mut stream: TcpStream,
    server_tx: Sender<Vec<u8>>,
    server_rx: Arc<Mutex<Receiver<Vec<u8>>>>,
    state: Arc<Mutex<ServerState>>,
    other_servers: Vec<String>,
    server_address: String,
) {
    // Buffer to hold incoming messages
    let mut buffer = [0; 1024];
    let bytes_read = stream.read(&mut buffer).expect("Failed to read data from client");

    // Check if the received message is "I want to send"
    let message = String::from_utf8_lossy(&buffer[..bytes_read]);
    if message.trim() == "I want to send" {
        println!("Received request from client middleware to send an image");

        // Start the election process
        let elected_server_ip = start_election(&state, &other_servers);

        if elected_server_ip == "self" {
            // If this server is selected, increase its load
            state.lock().expect("Failed to lock server state").increment_load();
            println!("Current load is {}; handling client's request.", state.lock().unwrap().current_load());

            // Send this server's address (including port) back to the client middleware
            stream.write_all(server_address.as_bytes()).expect("Failed to send IP to client middleware");

            // Flush the stream to ensure the address is sent before transitioning to data handling
            stream.flush().expect("Failed to flush stream");

            // Now transition to receiving and processing the image data on the same connection
            let mut data = Vec::new();
            stream.read_to_end(&mut data).expect("Failed to read image data from client middleware");
            println!("Image data received from client.");

            // Send image data to the server for encryption
            server_tx.send(data).expect("Failed to send data to server");

            // Receive the encrypted data from the server
            let encrypted_data = {
                let server_rx = server_rx.lock().expect("Failed to lock server receiver");
                server_rx.recv().expect("Failed to receive encrypted data from server")
            };

            // Send encrypted data back to client middleware
            stream.write_all(&encrypted_data).expect("Failed to send encrypted data to client middleware");

            // Decrease load after processing is complete
            state.lock().expect("Failed to lock server state").decrement_load();
        } else {
            println!("Election won by server at IP: {}", elected_server_ip);
        }
    }
}

// Function to start the election process based on the current load
fn start_election(state: &Arc<Mutex<ServerState>>, other_servers: &[String]) -> String {
    let current_load = state.lock().expect("Failed to lock server state").current_load();
    let mut elected_server_ip = "self".to_string();
    let mut lowest_load = current_load;

    for server_address in other_servers {
        // Send "load request" message to each other server to get their current load
        if let Ok(mut stream) = TcpStream::connect(server_address) {
            stream.write_all(b"load request").expect("Failed to send load request");

            // Receive load information from the server
            let mut buffer = [0; 1024];
            let bytes_read = stream.read(&mut buffer).expect("Failed to read load from other server");
            let server_load: u32 = String::from_utf8_lossy(&buffer[..bytes_read]).trim().parse().unwrap_or(u32::MAX);

            // Determine if this server has the lowest load
            if server_load < lowest_load {
                lowest_load = server_load;
                elected_server_ip = server_address.clone();
            }
        }
    }
    elected_server_ip
}


// Function to listen for load requests from other servers and respond with current load
fn listen_for_load_requests(address: String, state: Arc<Mutex<ServerState>>) {
    let listener = TcpListener::bind(address).expect("Failed to bind load listener address");
    for stream in listener.incoming() {
        if let Ok(mut stream) = stream {
            let mut buffer = [0; 1024];
            if let Ok(bytes_read) = stream.read(&mut buffer) {
                let request = String::from_utf8_lossy(&buffer[..bytes_read]);
                if request.trim() == "load request" {
                    // Get the current load and send it back as a response
                    let load = state.lock().expect("Failed to lock state for load retrieval").current_load();
                    let response = load.to_string();
                    stream.write_all(response.as_bytes()).expect("Failed to send load response");
                }
            }
        }
    }
}

