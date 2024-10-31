use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc::Sender, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use bincode;



// Define a struct for requests, including a unique ID and the image data
#[derive(Clone, Serialize, Deserialize)]
pub struct ImageRequest {
    pub client_ip : String,
    pub request_id: String,
    pub image_data: Vec<u8>,
}


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

pub async fn run_server_middleware(
    server_address: String, 
    load_request_port: u16, 
    server_tx: Sender<Vec<u8>>, 
    server_rx: Arc<Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>>
) {
    let listener = TcpListener::bind(&server_address).await.expect("Could not bind to server address");
    let state = Arc::new(Mutex::new(ServerState::new()));

    // Start a task to listen for load requests from other servers
    let load_listener_state = Arc::clone(&state);
    let load_listener_address = format!("0.0.0.0:{}", load_request_port);
    task::spawn(listen_for_load_requests(load_listener_address, load_listener_state));

    while let Ok((stream, _)) = listener.accept().await {
        let server_tx = server_tx.clone();
        let server_rx = Arc::clone(&server_rx);
        let state = Arc::clone(&state);

        task::spawn(async move {
            handle_connection(stream, server_tx, server_rx, state).await;
        });
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    server_tx: Sender<Vec<u8>>,
    server_rx: Arc<Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>>,
    state: Arc<Mutex<ServerState>>
) {
    // Buffer to hold incoming messages
    let mut buffer = [0; 1024];
    let bytes_read = stream.read(&mut buffer).await.expect("Failed to read data from client");

    // Check if the received message is "I want to send"
    let message = String::from_utf8_lossy(&buffer[..bytes_read]);
    if message.trim() == "I want to send" {
        println!("Received request from client middleware to send an image");

        // Increment load
        state.lock().await.increment_load();
        println!("Current load is {}; handling client's request.", state.lock().await.current_load());

        // Send the server's address back to the client middleware
        stream.write_all(b"self").await.expect("Failed to send IP to client middleware");

        // Now transition to receiving and processing the image data
        let mut data = Vec::new();
        stream.read_to_end(&mut data).await.expect("Failed to read image data from client middleware");
        let image_request: ImageRequest = bincode::deserialize(&data).expect("Failed to deserialize ImageRequest");
        println!("Request {} received from client {}.", image_request.request_id, image_request.client_ip);
        
        // Send image data to the server for encryption
        server_tx.send(image_request.image_data).await.expect("Failed to send data to server");

        // Receive the encrypted data from the server
        let encrypted_data = {
            let mut server_rx = server_rx.lock().await;
            server_rx.recv().await.expect("Failed to receive encrypted data from server")
        };

        // Send encrypted data back to client middleware
        stream.write_all(&encrypted_data).await.expect("Failed to send encrypted data to client middleware");

        // Decrease load after processing is complete
        state.lock().await.decrement_load();
    }
}

// Function to listen for load requests from other servers and respond with current load
async fn listen_for_load_requests(address: String, state: Arc<Mutex<ServerState>>) {
    let listener = TcpListener::bind(address).await.expect("Failed to bind load listener address");
    while let Ok((mut stream, _)) = listener.accept().await {
        let mut buffer = [0; 1024];
        if let Ok(bytes_read) = stream.read(&mut buffer).await {
            let request = String::from_utf8_lossy(&buffer[..bytes_read]);
            if request.trim() == "load request" {
                // Get the current load and send it back as a response
                let load = state.lock().await.current_load();
                let response = load.to_string();
                stream.write_all(response.as_bytes()).await.expect("Failed to send load response");
            }
        }
    }
}
