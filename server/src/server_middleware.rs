use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc::Sender, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use bincode;
use sysinfo::System;
use futures::future::join_all;
use std::collections::HashMap;
use std::time::{Duration, Instant}; 
use tokio::time::sleep;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use std::process;


#[derive(Clone, Serialize, Deserialize)]
pub struct LightMessage {
    pub client_ip : String,
    pub request_id: String,
    pub message: String,
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
        system.refresh_cpu_all();
        let cpu_utilization = system.global_cpu_usage();
        //self.id = 0.2f32 * load as f32 + 0.8f32 * cpu_utilization;
        self.id = load as f32;
    }

    fn get_id(&self) -> f32 {
        self.id
    }
}

// A structure to hold the server's state, including its load.
struct ServerState {
    load: u32,
    handled_requests: HashMap<(String, String), Instant>,
}

impl ServerState {
    fn new() -> Self {
        // Initialize the load with a random number between 1 and 50
        let mut rng = rand::thread_rng();
        let initial_load = rng.gen_range(1..=10); // Random load between 1 and 50

        ServerState {
            load: initial_load,
            handled_requests: HashMap::new(),
        }
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

    fn add_request(&mut self, client_ip: String, request_id: String) {
        self.handled_requests
            .insert((client_ip, request_id), Instant::now());
    }

    fn has_request(&self, client_ip: &String, request_id: &String) -> bool {
        self.handled_requests
            .contains_key(&(client_ip.clone(), request_id.clone()))
    }

    fn clean_old_requests(&mut self) {
        let now = Instant::now();
        let timeout = Duration::from_secs(120); // 2 minutes
        self.handled_requests
            .retain(|_, &mut timestamp| now.duration_since(timestamp) < timeout);
    }
}
pub async fn run_server_middleware(
    server_address: String,
    election_address: String,
    server_tx: Sender<Vec<u8>>,
    server_rx: Arc<Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>>,
    other_server_election_addresses: Vec<String>,
) {
    let listener = TcpListener::bind(&server_address)
        .await
        .expect("Could not bind to server address");
    let state = Arc::new(Mutex::new(ServerState::new()));

    // Start a task to listen for election messages
    let election_listener_state = Arc::clone(&state);
    //let election_listener_address = format!("0.0.0.0:{}", election_port);
    task::spawn(listen_for_election_messages(
        election_address,
        election_listener_state,
    ));

    // Start a task to clean old requests periodically
    let state_for_cleanup = Arc::clone(&state);
    task::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(200)).await; //adjust time
            state_for_cleanup.lock().await.clean_old_requests();
        }
    });

    let rng = Arc::new(Mutex::new(StdRng::seed_from_u64(process::id() as u64)));

    while let Ok((stream, _)) = listener.accept().await {
        let server_tx = server_tx.clone();
        let server_rx = Arc::clone(&server_rx);
        let state = Arc::clone(&state);
        let other_server_election_addresses = other_server_election_addresses.clone();
        let rng = Arc::clone(&rng);

        task::spawn(async move {
            handle_connection(
                stream,
                server_tx,
                server_rx,
                state,
                other_server_election_addresses,
                rng
            )
            .await;
        });
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    server_tx: Sender<Vec<u8>>,
    server_rx: Arc<Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>>,
    state: Arc<Mutex<ServerState>>,
    other_server_election_addresses: Vec<String>,
    mut rng: Arc<tokio::sync::Mutex<StdRng>>,
) {
 //   println!("Beginning handle_connection");
  //  let mut message = Vec::new();
    let mut light_buffer = [0; 1024];
    stream
        .read(&mut light_buffer)
        .await
        .expect("Failed to read light message data from client middleware");

    // check the if condition only when deserialization is successful
    if let Ok(light_message) = bincode::deserialize::<LightMessage>(&light_buffer) {
        println!("Received message: {}", light_message.message);
        if light_message.message == "I want to send" {
            println!(
                "Request {} received from client {}.",
                light_message.request_id, light_message.client_ip
            );

            // Check if the request is already being handled
            if state
                .lock()
                .await
                .has_request(&light_message.client_ip, &light_message.request_id)
            {
                println!(
                    "Request {} from client {} is already being handled.",
                    light_message.request_id, light_message.client_ip
                );
                return;
            }
	        //Delay before election
            let d = Duration::from_millis(rng.lock().await.gen_range(500..=10000));
            println!("Delaying before election {} with delay {}", process::id(), d.as_millis());
            sleep(d).await;
            // Initiate election
            let is_elected: bool = initiate_election(
                state.clone(),
                other_server_election_addresses.clone(),
                light_message.client_ip.clone(),
                light_message.request_id.clone(),
            )
            .await;

            if !is_elected {
                println!("Server is not elected to handle the request {}.", light_message.request_id);
                return;
            }

            // Increment load
            state.lock().await.increment_load();
            //println!(
            //    "Current load is {}; handling client's request.",
            //    state.lock().await.current_load()
            //);
            // Send the server's address back to the client middleware
            stream
                .write_all(b"self")
                .await
                .expect("Failed to send IP to client middleware");

            // Now transition to receiving and processing the image data
            let mut data = Vec::new();
            stream
                .read_to_end(&mut data)
                .await
                .expect("Failed to read image data from client middleware");

            // Send image data to the server for encryption
            server_tx
                .send(data)
                .await
                .expect("Failed to send data to server");

            // Receive the encrypted data from the server
            let encrypted_data = {
                let mut server_rx = server_rx.lock().await;
                server_rx
                    .recv()
                    .await
                    .expect("Failed to receive encrypted data from server")
            };

            // Send encrypted data back to client middleware
            stream
                .write_all(&encrypted_data)
                .await
                .expect("Failed to send encrypted data to client middleware");

            // Decrease load after processing is complete
            state.lock().await.decrement_load();
        }
    }
}


async fn initiate_election(
    state: Arc<Mutex<ServerState>>,
    other_server_election_addresses: Vec<String>,
    client_ip: String,
    request_id: String,
) -> bool {
    // Create a new system for getting CPU utilization
    let mut system = sysinfo::System::new_all();
    let load = state.lock().await.current_load();
    let mut server_id = ServerId::new();
    server_id.calculate_id(load, &mut system);
    let own_id = server_id.get_id();

    println!("Initiating election with ID: {} for request: {}", own_id, request_id);

    // Prepare futures for sending election messages
    let mut futures = Vec::new();

    for address in other_server_election_addresses.iter() {
        let address = address.clone();
        let election_message = format!("ELECTION:{};{};{}", own_id, client_ip, request_id);
        // Create a future for each server
        let future = async move {
            // Attempt to connect to the server
            match TcpStream::connect(&address).await {
                Ok(mut stream) => {
                    // Send election message
                    if let Err(e) = stream.write_all(election_message.as_bytes()).await {
                        println!("Failed to send election message to {}: {}", address, e);
                        return None;
                    }
                    // Set a timeout for reading response
                    let mut buffer = [0; 1024];
                    match tokio::time::timeout(
                        std::time::Duration::from_millis(1000), // timeout; maybe increase it
                        stream.read(&mut buffer),
                    )
                    .await
                    {
                        Ok(Ok(bytes_read)) => {
                            let response = String::from_utf8_lossy(&buffer[..bytes_read]);
                            if response.trim() == "OK" || response.trim() == "ALREADY_HANDLED" {
                                Some(response.trim().to_string())
                            } else {
                                None
                            }
                        }
                        _ => {
                            println!("No response or error from server {}", address);
                            // No response or error
                            None
                        }
                    }
                }
                Err(e) => {
                    println!("Failed to connect to {}: {}", address, e);
                    None
                }
            }
        };
        futures.push(future);
    }

    // Wait for all election messages to complete
    let results = join_all(futures).await;

    // Check if any server responded with "OK" or "ALREADY_HANDLED"
    let mut should_handle_request = true;
    for response in results {
        if let Some(res) = response {
            println!("Herrrre Received response from other servers : {}", res);
            if res == "OK" {
                //println!("Received OK from another server. Not the leader.");
                should_handle_request = false;
                break;
            } else if res == "ALREADY_HANDLED" {
                //println!("Request already handled by another server.");
                should_handle_request = false;
                break;
            }
        }
    }

    if should_handle_request {
        // We are the leader
        println!(
            "We are the leader for request {} from client {}.",
            request_id, client_ip
        );

        // Add the request to our handled_requests
        state
            .lock()
            .await
            .add_request(client_ip.clone(), request_id.clone());

        // Send LEADER message to other servers
        let leader_message = format!("LEADER:{}:{}", client_ip, request_id);
        let mut leader_futures = Vec::new();

        for address in other_server_election_addresses.iter() {
            let address = address.clone();
            let leader_message = leader_message.clone();
            let future = async move {
                match TcpStream::connect(&address).await {
                    Ok(mut stream) => {
                        if let Err(e) = stream.write_all(leader_message.as_bytes()).await {
                            println!("Failed to send LEADER message to {}: {}", address, e);
                        }
                    }
                    Err(e) => {
                        println!("Failed to connect to {}: {}", address, e);
                    }
                }
            };
            leader_futures.push(future);
        }
        // Send LEADER messages in parallel
        join_all(leader_futures).await;
    }

    should_handle_request
}

// Function to listen for election messages from other servers
async fn listen_for_election_messages(address: String, state: Arc<Mutex<ServerState>>) {
    let listener = TcpListener::bind(address)
        .await
        .expect("Failed to bind election listener address");
    while let Ok((mut stream, _)) = listener.accept().await {
        let mut buffer = [0; 1024];
        if let Ok(bytes_read) = stream.read(&mut buffer).await {
            let message = String::from_utf8_lossy(&buffer[..bytes_read]);
            if message.starts_with("ELECTION:") {
                // Extract sender_id, client_ip, request_id
                let parts: Vec<&str> = message["ELECTION:".len()..].trim().split(';').collect();
                //println!("Received election message: {}", message);
                print!("parts: {}", parts.len());
                if parts.len() == 3 {
                    let sender_id: f32 = parts[0].parse().unwrap_or(f32::MAX);
                    let client_ip = parts[1].to_string();
                    let request_id = parts[2].to_string();
                    //print a message with the sender_id, client_ip and request_id
                    //println!("Received election message from ID {}. Client IP: {}. Request ID: {}.", sender_id, client_ip, request_id);

                    // Check if we have already handled this request
                    if state
                        .lock()
                        .await
                        .has_request(&client_ip, &request_id)
                    {
                        // Send "ALREADY_HANDLED"
                        if let Err(e) = stream.write_all(b"ALREADY_HANDLED").await {
                            println!("Failed to send ALREADY_HANDLED to election message: {}", e);
                        }
                        continue;
                    }

                    // Calculate our own ID
                    let mut system = sysinfo::System::new_all();
                    let load = state.lock().await.current_load();
                    let mut server_id = ServerId::new();
                    server_id.calculate_id(load, &mut system);
                    let own_id = server_id.get_id();

                    println!(
                        "Received election message from ID {}. Our ID is {}. Request ID is {}.",
                        sender_id, own_id, request_id
                    );

                    // Compare IDs
                    if own_id < sender_id {
                        // Our ID is lower, reply "OK"
                        if let Err(e) = stream.write_all(b"OK").await {
                            println!("Failed to send OK to election message: {}", e);
                        }
                    }
                    // If our ID is higher, do not respond
                }else{
                    println!("Invalid election message format");
                }
            } else if message.starts_with("LEADER:") {
                // Extract client_ip and request_id
                let parts: Vec<&str> = message["LEADER:".len()..].trim().split(';').collect();
                if parts.len() == 2 {
                    let client_ip = parts[0].to_string();
                    let request_id = parts[1].to_string();

                    // Add the request to our handled_requests
                    state
                        .lock()
                        .await
                        .add_request(client_ip.clone(), request_id.clone());
                    println!(
                        "Added request {} from client {} to handled requests.",
                        request_id, client_ip
                    );
                }
            }
        }
    }
}
