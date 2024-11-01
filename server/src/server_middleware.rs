use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc::Sender, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use sysinfo::System;
use rand::Rng;
use std::time::{SystemTime, UNIX_EPOCH};
use bincode;



// Define a struct for requests, including a unique ID and the image data
#[derive(Clone, Serialize, Deserialize)]
pub struct ImageRequest {
    pub client_ip : String,
    pub request_id: String,
    pub image_data: Vec<u8>,
}

// Define an ElectionMessage struct that includes the sender's ID
#[derive(Serialize, Deserialize)]
struct ElectionMessage {
    sender_id: f32,
}

// A structure to hold the server's id, a weighted sum of its load and cpu utilization.
struct ServerId {
    id: f32,
}

impl ServerId {
    fn new() -> Self {
        ServerId { id: 0.0 }
    }

    fn calculate_id(&mut self, load: u32, system: &mut System) {
        // system.refresh_cpu_all();
        // let cpu_utilization = system.global_cpu_usage();
        // self.id = 0.2f32 * load as f32 + 0.8f32 * cpu_utilization;
        // let mut rng = rand::thread_rng(); // Get a random number generator
        // let random_f32: f32 = rng.gen();
        self.id = load as f32;
        println!("Server ID: {}", self.id);
    }

    fn get_id(&self) -> f32 {
        self.id
    }
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
    server_rx: Arc<Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>>,
    other_servers: Vec<String>
) {
    let listener = TcpListener::bind(&server_address).await.expect("Could not bind to server address");
    let state = Arc::new(Mutex::new(ServerState::new()));
    let server_id = Arc::new(Mutex::new(ServerId::new()));
    let system = Arc::new(Mutex::new(System::new()));

    // Start a task to listen for load requests from other servers
    let load_listener_state = Arc::clone(&state);
    let load_listener_address = format!("0.0.0.0:{}", load_request_port);
    task::spawn(listen_for_load_requests(load_listener_address, load_listener_state));

    while let Ok((stream, _)) = listener.accept().await {
        let server_tx = server_tx.clone();
        let server_rx = Arc::clone(&server_rx);
        let state = Arc::clone(&state);
        let server_id = Arc::clone(&server_id);
        let system = Arc::clone(&system);
        let other_servers = other_servers.clone();

        task::spawn(async move {
            handle_connection(stream, server_tx, server_rx, state, server_id, system, other_servers).await;
        });
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    server_tx: Sender<Vec<u8>>,
    server_rx: Arc<Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>>,
    state: Arc<Mutex<ServerState>>,
    server_id: Arc<Mutex<ServerId>>,
    system: Arc<Mutex<System>>,
    other_servers: Vec<String>,
) {
    // Buffer to hold incoming messages
    let mut buffer = [0; 1024];
    let bytes_read = stream.read(&mut buffer).await.expect("Failed to read data from client");

    // Check if the received message is "I want to send"
    let message = String::from_utf8_lossy(&buffer[..bytes_read]);
    if message.trim() == "I want to send" {
        println!("Received request from client middleware to send an image");

        // Calculate server ID
        {
            let mut server_id = server_id.lock().await;
            let load = state.lock().await.current_load();
            let mut system = system.lock().await;
            server_id.calculate_id(load, &mut *system);
        }
        let my_id = server_id.lock().await.get_id();

        // Initiate leader election
        println!("server_id: {} is initiating leader election", my_id);
        let lower_id_servers = send_election_message(&other_servers, my_id).await;
        println!("Lower id servers: {:?}", lower_id_servers);

        if lower_id_servers.is_empty() {
            // Increment load
            println!("server_id: {} is the leader", my_id);
            state.lock().await.increment_load();

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
        } else {
            println!("server_id: {} is not the leader", my_id);
            return;
        }
    } else if  message.trim() == "election" {
        println!("Received election message from another server");
        let mut data = Vec::new();
        stream.read_to_end(&mut data).await.expect("Failed to read election message data");
        let election_message: ElectionMessage = bincode::deserialize(&data).expect("Failed to deserialize ElectionMessage");
        let sender_id = election_message.sender_id;

        // Recalculate our own id
        let mut server_id = server_id.lock().await;
        let load = state.lock().await.current_load();
        let mut system = system.lock().await;
        server_id.calculate_id(load, &mut *system);
        let my_id = server_id.get_id();

        if my_id < sender_id {
            stream.write_all(b"leader").await.expect("Failed to send leader message to client middleware");
        } else {
            stream.write_all(b"not_leader").await.expect("Failed to send not leader message to client middleware");

        } 
    }
}

async fn send_election_message(other_servers: &Vec<String>, my_id: f32) -> Vec<String> {
    let mut lower_id_servers = Vec::new();
    println!("other_servers: {:?}", other_servers);
    for server in other_servers {
        println!("Attempting to connect to server: {}", server);
        if let Ok(mut stream) = TcpStream::connect(server).await {
            println!("Connected to server: {}", server);
            // Prepare the election message with our id
            let election_message = ElectionMessage { sender_id: my_id };
            let message_bytes = bincode::serialize(&election_message).expect("Failed to serialize ElectionMessage");
            // Send a header to specify that this is an election message
            stream.write_all(b"election").await.expect("Failed to send election message to server");
            // Then send the serialized ElectionMessage
            stream.write_all(&message_bytes).await.expect("Failed to send election message to server");
            println!("Sent election message to server: {}", server);
            let mut buffer = [0; 1024];
            if let Ok(bytes_read) = stream.read(&mut buffer).await {
                let message = String::from_utf8_lossy(&buffer[..bytes_read]);
                println!("Received message from server {}: {}", server, message);
                if message.trim() == "leader" {
                    lower_id_servers.push(server.clone());
                }
            }
        } else {
            println!("Failed to connect to server: {}", server);
        }
    }
    lower_id_servers
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
