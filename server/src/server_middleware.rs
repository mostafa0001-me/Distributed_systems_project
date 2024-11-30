use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc::Sender, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task;
use serde::{Serialize, Deserialize};
use bincode;
use futures::future::join_all;
use std::collections::HashMap;
use std::time::{Duration, Instant}; 
use sysinfo::{System, RefreshKind, CpuRefreshKind};
use rand::{Rng, rngs::StdRng, SeedableRng};
use std::process;
use std::thread;
use std::sync::Arc;
use serde_json;
//use std::fs::{File, OpenOptions};
// use std::io::Write; please stick with tokio
use nanoid::nanoid;

const ALPHABET_IDS: [char; 62] = [
    '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', 
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
];


#[derive(Clone, Serialize, Deserialize)]
pub enum Request {
    SignUp(SignUpRequest),
    SignIn(SignInRequest),
    SignOut(SignOutRequest),
    ImageRequest(ImageRequest),
    HandShake(HandShakeRequest),
    DOS(DOSRequest),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SignUpRequest {
    pub client_ip: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SignInRequest {
    pub client_id: String,
    pub client_ip: String,
    pub reply_back: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SignOutRequest {
    pub client_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ImageRequest {
    pub client_ip: String,
    pub request_id: String,
    pub image_data: Vec<u8>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct HandShakeRequest {
    pub client_ip: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct DOSRequest {
    pub client_id: String,
}

/// Struct representing am online client (helpful for DOS response)
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct OnlineClient {
    pub client_id: String,
    pub client_ip: String,
    pub images: Vec<String>, // Names of Images owned by the client
}

#[derive(Clone, Serialize, Deserialize)]
pub enum Response {
    SignUp {client_id: String},
    SignIn {success: bool},
    SignOut {success: bool},
    ImageResponse(ImageResponse),
    HandShake {success: bool},
    DOS {online_clients: Vec<OnlineClient>},
    Error {message: String},
}

#[derive(Clone, Serialize, Deserialize)]
pub struct LightMessage {
    pub client_ip : String,
    pub request_id: String,
    pub message: String,
}

// Assuming the response now also contains the request ID
#[derive(Clone, Serialize, Deserialize)]
pub struct ImageResponse {
    pub request_id: String,
    pub encrypted_image_data: Vec<u8>,
}

// A structure to hold the server's id, a weighted sum of its load and cpu utilization.
struct ServerId {
    id: u32,
}

impl ServerId {
    fn new() -> Self {
        ServerId { id: 0 }
    }

    fn calculate_id(&mut self, load: u32) {
        let mut system = System::new_with_specifics(
            RefreshKind::new().with_cpu(CpuRefreshKind::everything()),
        );

        // Wait a bit because CPU usage is based on diff.
        thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        system.refresh_cpu_all();
     //   let cpu_utilization = system.global_cpu_usage();
        let mut cpu_utilization = 0.0;
        for cpu in system.cpus() {
            cpu_utilization += cpu.cpu_usage();
        }
        cpu_utilization /= system.cpus().len() as f32;
        cpu_utilization *= 100 as f32; //make it bigger for easier comparison
        self.id = cpu_utilization as u32;
       // self.id = 0.2f32 * load as f32 + 0.8f32 * cpu_utilization;
        //self.id = load as f32;
    }

    fn get_id(&self) -> u32 {
        self.id
    }
}

// A structure to hold the server's state, including its load.
struct ServerState {
    load: u32,
    handled_requests: HashMap<(String, String), Instant>,
    requests_received: HashMap<(String, String), u32>, // request ID to load
}

impl ServerState {
    fn new() -> Self {
        ServerState {
            load: 0,
            handled_requests: HashMap::new(),
            requests_received: HashMap::new(),
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

    fn add_request_handled(&mut self, client_ip: String, request_id: String) {
        self.handled_requests
            .insert((client_ip, request_id), Instant::now());
    }

    fn has_request_handled(&self, client_ip: &String, request_id: &String) -> bool {
        self.handled_requests
            .contains_key(&(client_ip.clone(), request_id.clone()))
    }

    fn add_request_received(&mut self, client_ip: String, request_id: String, load: u32) {
        self.requests_received
            .insert((client_ip, request_id), load);
    }
    fn has_request_received(&self, client_ip: &String, request_id: &String) -> bool {
        self.requests_received
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
    let election_listener_address = election_address.clone();
    task::spawn(listen_for_election_messages(
        election_listener_address,
        election_listener_state,
    ));

    // // Start a task to clean old requests periodically
    // let state_for_cleanup = Arc::clone(&state);
    // task::spawn(async move {
    //     loop {
    //         tokio::time::sleep(Duration::from_secs(200)).await; //adjust time
    //         state_for_cleanup.lock().await.clean_old_requests();
    //     }
    // });

    let rng = Arc::new(Mutex::new(StdRng::seed_from_u64(process::id() as u64)));

    while let Ok((stream, _)) = listener.accept().await {
        let server_tx = server_tx.clone();
        let server_rx: Arc<Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>> = Arc::clone(&server_rx);
        let state = Arc::clone(&state);
        let other_server_election_addresses = other_server_election_addresses.clone();
        let rng = Arc::clone(&rng);

        let election_listener_address2 = election_address.clone();
        let mut dos = DoS::new().await;
        task::spawn(async move {
            handle_connection(
                stream,
                server_tx,
                server_rx,
                state,
                election_listener_address2,
                other_server_election_addresses,
                rng,
                &mut dos
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
    my_election_address: String,
    other_server_election_addresses: Vec<String>,
    rng: Arc<tokio::sync::Mutex<StdRng>>,
    dos: &mut DoS
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

            thread::sleep( Duration::from_millis(rng.lock().await.gen_range(20..=100))); 
            // Check if the request is already being handled
            if state
                .lock()
                .await
                .has_request_handled(&light_message.client_ip, &light_message.request_id)
            {
                println!(
                    "Request {} from client {} is already being handled.",
                    light_message.request_id, light_message.client_ip
                );
                return;
            }
	        // //Delay before election
            let d = Duration::from_millis(rng.lock().await.gen_range(100..=500));
            println!("Delaying before election {} with delay {}", process::id(), d.as_millis());
           // tokio::time::sleep(d).await;
            thread::sleep(d);
            // Initiate election
            let is_elected: bool = initiate_election(
                state.clone(),
                my_election_address.clone(),
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
            println!(
                "Current load is {}; handling client's request.",
                state.lock().await.current_load()
            );
            // Send the server's address back to the client middleware
            stream
                .write_all(b"self")
                .await
                .expect("Failed to send IP to client middleware");
            // Zezooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooo
            // Now transition to receiving and processing the image data

            let mut buffer = Vec::new();
            match stream.read_to_end(&mut buffer).await {
                Ok(_) => {    
                    // Attempt to deserialize the request
                    match serde_json::from_slice::<Request>(&buffer) {
                        Ok(request) => {
                            match request {
                                Request::SignUp(req) => {
                                    println!("In server middleware request {}", req.client_ip);
                                    let response: Response = dos.register_client(req.client_ip).await;
                                    match &response {
                                        Response::SignUp { client_id } => {
                                            println!("In server middleware response {}", client_id)
                                        },
                                        _ => {},
                                    }
                                    // Serialize and send the response back to the client
                                    let serialized_response = serde_json::to_string(&response).unwrap();
                                    println!("Serialized response {}", serialized_response);
                                    stream.write_all(&serialized_response.as_bytes())
                                    .await
                                    .expect("Failed to send the client id back to the client middleware");
                                    // // Decrease load after processing is complete
                                    state.lock().await.decrement_load();
                                },
                                Request::SignIn(req) => {
                                    let response = dos.sign_in_client(req.client_id, req.client_ip).await;
                                    match &response {
                                        Response::SignIn { success } => {
                                            println!("In server middleware response for singn in {}", success)
                                        },
                                        _ => {},
                                    }
                                    // Serialize and send the response back to the client
                                    let serialized_response = serde_json::to_string(&response).unwrap();
                                    stream.write_all(&serialized_response.as_bytes())
                                    .await
                                    .expect("Failed to send the client id back to the client middleware");
                                    // Decrease load after processing is complete
                                    state.lock().await.decrement_load();
                                },
                                Request::SignOut(req) =>{
                                    let response = dos.sign_out_client(req.client_id).await;
                                    match &response {
                                        Response::SignOut { success } => {
                                            println!("In server middleware response for sign out {}", success)
                                        },
                                        _ => {},
                                    }
                                    // Serialize and send the response back to the client
                                    let serialized_response = serde_json::to_string(&response).unwrap();
                                    stream.write_all(&serialized_response.as_bytes())
                                    .await
                                    .expect("Failed to send the client id back to the client middleware");
                                    // Decrease load after processing is complete
                                    state.lock().await.decrement_load();
                                }
                                Request::DOS(req) => {
                                    let response = dos.get_online_clients(req.client_id).await;
                                    match &response {
                                        Response::DOS { online_clients } => {
                                            println!("In server middleware response for DOS. Returned {:?}", online_clients)
                                        },
                                        _ => {},
                                    }
                                    // Serialize and send the response back to the client
                                    let serialized_response = serde_json::to_string(&response).unwrap();
                                    stream.write_all(&serialized_response.as_bytes())
                                    .await
                                    .expect("Failed to send the client id back to the client middleware");
                                    // Decrease load after processing is complete
                                    state.lock().await.decrement_load();
                                },
                                Request::HandShake( _req) => { // Not needed. was a trial for a null request. 
                                    // let response = Response::HandShake { success: true };
                                    // // Serialize and send the response back to the client
                                    // match &response {
                                    //     Response::HandShake { success } => {
                                    //         //nothing to do
                                    //     },
                                    //     _ => {},
                                    // }
                                    // let serialized_response = serde_json::to_string(&response).unwrap();
                                    // stream.write_all(&serialized_response.as_bytes())
                                    // .await
                                    // .expect("Failed to send the client id back to the client middleware");
                                    // // Decrease load after processing is complete
                                    // state.lock().await.decrement_load();
                                },
                                Request::ImageRequest(data) => {
                                    println!("I am an image request");
                                    // Send image data to the server for encryption
                                    server_tx
                                    .send(data.image_data)
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

                                    // Serialize the LightRequest
                                    let response = Response::ImageResponse(ImageResponse{
                                        request_id: light_message.request_id.clone(),
                                        encrypted_image_data: encrypted_data,
                                    });

                                    // Serialize and send the response back to the client
                                    let serialized_response = serde_json::to_string(&response).unwrap();
                                    stream.write_all(&serialized_response.as_bytes())
                                    .await
                                    .expect("Failed to send the client id back to the client middleware");
                                    // Decrease load after processing is complete
                                    state.lock().await.decrement_load();
                                },
                                _ => {},
                            }
                        },
                        Err(err) => {
                            eprintln!("Failed to deserialize request: {}", err);
                            let error_response = Response::Error { message: "Invalid request format".to_string()};
                            let serialized_response = serde_json::to_vec(&error_response).unwrap();
                            if let Err(err) = stream.write_all(&serialized_response).await {
                                eprintln!("Failed to send error response: {}", err);
                            }
                        }
                    }
                }
                Err(err) => {
                    eprintln!("Failed to read from socket: {}", err);
                }
            }
        }
    }
}


async fn initiate_election(
    state: Arc<Mutex<ServerState>>,
    my_election_address: String,
    other_server_election_addresses: Vec<String>,
    client_ip: String,
    request_id: String,
) -> bool {
    // Create a new system for getting CPU utilization
    //let mut system = sysinfo::System::new_all();
    let own_id = if state.lock().await.has_request_received(&client_ip, &request_id) {
        state.lock().await.requests_received[&(client_ip.clone(), request_id.clone())]
    } else {
        // Acquire the mutex lock first
        let mut state_guard = state.lock().await;
        println!(
            "In initiate election, Own ID not found for request {} from client {}.",
            request_id, client_ip
        );
        // Calculate our own ID
        let load = state_guard.current_load();
        let mut server_id = ServerId::new();
        server_id.calculate_id(load);
        let own_id = server_id.get_id();
        state_guard.add_request_received(client_ip.clone(), request_id.clone(), own_id);
        own_id
    };    
    println!("Initiating election for request {} with ID: {}", request_id, own_id);

    // Prepare futures for sending election messages
    let mut futures = Vec::new();

    for address in other_server_election_addresses.iter() {
        let address = address.clone();
        let election_message = format!("ELECTION:{};{};{};{}", own_id, client_ip, request_id, my_election_address);
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
                        std::time::Duration::from_millis(4000), // timeout; maybe increase it
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
                println!("Received OK from another server. Not the leader.");
                should_handle_request = false;
                break;
            } else if res == "ALREADY_HANDLED" {
                println!("Request already handled by another server.");
                should_handle_request = false;
                break;
            }
        }
    }

    //wait before sending leader message
    thread::sleep( Duration::from_millis(100)); 

    if should_handle_request {
        if state
        .lock()
        .await
        .has_request_handled(&client_ip.clone(), &request_id.clone())
        {
        println!(
            "Request {} from client {} is already being handled.",
            request_id, client_ip
        );
        return false;
        }

        // We are the leader
        println!(
            "I am the leader for request {} from client {}.",
            request_id, client_ip
        );

        // Add the request to our handled_requests
        state
            .lock()
            .await
            .add_request_handled(client_ip.clone(), request_id.clone());

        thread::sleep( Duration::from_millis(200)); 
        // Send LEADER message to other servers
        let leader_message = format!("LEADER:{};{};{}", client_ip, request_id, my_election_address);
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
    let listener = TcpListener::bind(address.clone())
        .await
        .expect("Failed to bind election listener address");
    while let Ok((mut stream, _)) = listener.accept().await {
        let mut buffer = [0; 1024];
        if let Ok(bytes_read) = stream.read(&mut buffer).await {
            let message = String::from_utf8_lossy(&buffer[..bytes_read]);
            if message.starts_with("ELECTION:") {
                // Extract sender_id, client_ip, request_id
                let parts: Vec<&str> = message["ELECTION:".len()..].trim().split(';').collect();
                println!("Received election message: {}", message);
                print!("parts: {}", parts.len());
                if parts.len() == 4 {
                    let sender_id: u32 = parts[0].parse().unwrap();
                    let client_ip = parts[1].to_string();
                    let request_id = parts[2].to_string();
                    let sender_election_address = parts[3].to_string();
                    //print a message with the sender_id, client_ip and request_id
                    println!("Received election message from server {} with ID {}. Client IP: {}. Request ID: {}.", sender_election_address, sender_id, client_ip, request_id);

                    thread::sleep( Duration::from_millis(200)); 
                    // Check if we have already handled this request
                    if state
                        .lock()
                        .await
                        .has_request_handled(&client_ip, &request_id)
                    {
                        // Send "ALREADY_HANDLED"
                        if let Err(e) = stream.write_all(b"ALREADY_HANDLED").await {
                            println!("Failed to send ALREADY_HANDLED to election message: {}", e);
                        }
                        continue;
                    }

                    // Calculate our own ID
                    // let load = state.lock().await.current_load();
                    // let mut server_id = ServerId::new();
                    // server_id.calculate_id(load);
                    // let own_id = server_id.get_id();
                    let own_id = if state.lock().await.has_request_received(&client_ip, &request_id) {
                        state.lock().await.requests_received[&(client_ip.clone(), request_id.clone())]
                    } else {
                        // Acquire the mutex lock first
                        let mut state_guard = state.lock().await;
                        println!(
                            "In hearing election, Own ID not found for request {} from client {}.",
                            request_id, client_ip
                        );
                        // Calculate our own ID
                        let load = state_guard.current_load();
                        let mut server_id = ServerId::new();
                        server_id.calculate_id(load);
                        let own_id = server_id.get_id();
                        state_guard.add_request_received(client_ip.clone(), request_id.clone(), own_id);
                        own_id
                    }; 

                    println!(
                        "Received election message for request {} from server with ID {}. Our ID is {}.",request_id, 
                        sender_id, own_id
                    );

                    // Compare IDs
                    println!("Comparing server IPS. Listener: {} Sender: {}", address, sender_election_address);
                    if (own_id < sender_id) || (own_id == sender_id && address < sender_election_address) {
                        // Our ID is lower, reply "OK"
                        println!("Our ID is lower. Replying OK for request {}.", request_id );
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
                if parts.len() == 3 {
                    let client_ip = parts[0].to_string();
                    let request_id = parts[1].to_string();
                    let sender_election_address = parts[2].to_string();


                    // Add the request to our handled_requests
                    state
                        .lock()
                        .await
                        .add_request_handled(client_ip.clone(), request_id.clone());
                    println!(
                        "Added request {} from client {} to handled requests which is handled by server {}.",
                        request_id, client_ip, sender_election_address
                    );
                }
            }
        }
    }
}

// /// Struct representing a downsampled image with a unique ID
// #[derive(Serialize, Deserialize, Debug)]
// struct Image {
//     id: u64,
//     image_data: Vec<u8>, // Placeholder for downsampled image data
// }

// Shared State of the Directory of Service (DoS)
pub struct DoS {
}

impl DoS {
    pub async  fn new() -> Self {
        tokio::fs::create_dir_all("DOS")
        .await
        .expect("Failed to create 'my_images' directory");
        DoS {}
    }
    
    // Registers a new client and assigns a unique ID.
    async fn register_client(&mut self, ip: String) -> Response {
        let mut client_id = nanoid!(8, &ALPHABET_IDS); // small unique_ID of 8 characters for easier testing.
        let mut file_path = format!("DOS/{}.txt", client_id);
        while does_file_exist(&file_path).await { // ensure id is not used in a previous session.
            client_id = nanoid!(8, &ALPHABET_IDS);
            file_path = format!("DOS/{}.txt", client_id);
        }
        // create the client file, and mark it as online and put its ip address.
        // The file name is the client id
        // first line is either 1,client_ip (if online) or 0,cliiet_ip (if offline)
        let content = format!("1,{}\n", ip);
        if let Err(err) = append_to_file(&file_path, &content).await {
            eprintln!("Failed to write client_id to file: {}", err);
        }
        Response::SignUp {client_id}
    }

    /// Signs in an existing client and marks it as online.
    async fn sign_in_client(&mut self, client_id: String, client_ip: String) -> Response {
        let file_path = format!("DOS/{}.txt", client_id);
        // check if the client id exists in the directory of service
        if !does_file_exist(&file_path).await {
            return Response::SignIn { success: false };
        }
        // read the file.
        let content = read_from_file(&file_path).await.unwrap();
        let lines = content.lines();
        // change first line to be 1,client_ip
        let mut new_content = format!("1,{}\n", client_ip);
        for line in lines.skip(1) {
            new_content.push_str(line);
        }
        if let Err(err) = write_to_file(&file_path, &new_content).await {
            eprintln!("Failed to write sign in to file: {}", err);
            Response::SignIn { success: false };
        }
        Response::SignIn { success: true } 
    }

    /// Signs out an existing client and marks it as offline.
    async fn sign_out_client(&mut self, client_id: String) -> Response {
        let file_path = format!("DOS/{}.txt", client_id);
        // check if the client id exists in the directory of service
        if !does_file_exist(&file_path).await {
            return Response::SignOut { success: false };
        }
        let content = read_from_file(&file_path).await.unwrap();
        let mut lines = content.lines();

        if let Some(first_line) = lines.next() {
            // Split the first line to extract the IP
            let mut parts = first_line.split(',');
            let _status = parts.next();
            let ip = parts.next().unwrap_or("");

            // Create new content with updated first line
            let mut new_content = format!("0,{}\n", ip);

            // Append the rest of the lines
            for line in lines {
                new_content.push_str(line);
                new_content.push('\n');
            }
            // Write the new content back to the file
            if let Err(err) = write_to_file(&file_path, &new_content).await {
                eprintln!("Failed to write sign out to file: {}", err);
                Response::SignOut { success: false };
            }
        }
       
        Response::SignOut { success: true }
    }

    // get online clients, their ips, and images
    async fn get_online_clients(&self, requester_id: String) -> Response {
        let mut online_clients = Vec::new();
        let mut clients_ids = Vec::new();
        if let Ok(mut entries) = tokio::fs::read_dir("DOS").await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if let Some(file_name) = path.file_name() {
                    if let Some(file_name) = file_name.to_str() {
                        if file_name.ends_with(".txt") {
                            clients_ids.push(file_name.to_string().replace(".txt", ""));
                        }
                    }
                }
            }
        }
        for client_id in clients_ids {
            if client_id == requester_id {
                continue; // don't return the requester info
            }
            let content = read_from_file(&format!("DOS/{}.txt", client_id)).await.unwrap();
            let mut lines = content.lines();
            if let Some(first_line) = lines.next() {
                let mut parts = first_line.split(',');
                let status = parts.next().unwrap_or(""); // 1 for online, 0 for offline
                let ip = parts.next().unwrap_or("");
                if status == "1" {
                    let mut images = Vec::new();
                    // images are in the second line seperated by commas
                    if let Some(second_line) = lines.next() {
                        let parts = second_line.split(',');
                        for part in parts {
                            images.push(part.to_string());
                        }
                    }
                    online_clients.push(OnlineClient {
                        client_id: client_id,
                        client_ip: ip.to_string(),
                        images,
                    });
                }
            }
        }
        Response::DOS { online_clients }
    }
}

/// Appends a string to a file, creating the file if it doesn't exist.
async fn append_to_file(file_path: &str, content: &str) -> tokio::io::Result<()> {
    let mut file = tokio::fs::OpenOptions::new()
    .create(true) // Create the file if it doesn't exist
    .append(true) // Append to the file
    .open(file_path)
    .await?;

    file.write_all(content.as_bytes()).await?; // Write the content to the file
    Ok(())
    }

async fn does_file_exist(file_path: &str) -> bool {
    tokio::fs::metadata(file_path).await.is_ok()
}

async fn read_from_file(file_path: &str) -> tokio::io::Result<String> {
    let mut file = tokio::fs::File::open(file_path).await?;
    let mut content = String::new();
    file.read_to_string(&mut content).await?;
    Ok(content)
}

async fn write_to_file(file_path: &str, content: &str) -> tokio::io::Result<()> {
    tokio::fs::write(file_path, content).await
}

// /// Checks if a given client ID exists in the file.
// fn client_id_exists_in_file(file_path: &str, client_id: &str) -> bool {
//     if let Ok(file) = File::open(file_path) {
//         let reader = BufReader::new(file);
//         for line in reader.lines() {
//             if let Ok(existing_id) = line {
//                 if existing_id.trim() == client_id {
//                     return true;
//                 }
//             }
//         }
//     }
//     false
// }
