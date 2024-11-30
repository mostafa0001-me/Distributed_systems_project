// src/client.rs

use tokio::sync::{mpsc, Mutex};
use tokio::fs;
use uuid::Uuid;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::fmt::format;
use std::time::Instant;
use once_cell::sync::Lazy;
use std::io::{self};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use bincode;
use tokio::fs::OpenOptions;
use tokio::net::{TcpListener, TcpStream};
use image::DynamicImage;
use steganography::decoder::Decoder;
use std::sync::Arc;
use tokio::task;

use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::client;

#[derive(Clone, Serialize, Deserialize)]
pub enum Request {
    SignUp(SignUpRequest),
    SignIn(SignInRequest),
    SignOut(SignOutRequest),
    ImageRequest(ImageRequest),
    DOS(DOSRequest),
    HandShake(HandShakeRequest),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SignUpRequest {
    pub client_ip: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SignInRequest {
    pub client_id: String,
    pub client_ip: String, // to keep track if the ip has changed
    pub reply_back: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SignOutRequest {
    pub client_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ImageRequest { 
    pub client_id: String,
    pub request_id: String,
    pub image_name: String, // we assume it is unique per clinet.
    pub image_data: Vec<u8>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct DOSRequest {
    pub client_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HandShakeRequest {
    pub client_ip: String,
}
/// Struct representing am online client (helpful for DOS response)
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct OnlineClient {
    pub client_id: String,
    pub client_ip: String,
    pub images: Vec<String>, // Names of Images owned by the client
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum Response {
    SignUp(SignUpResponse),
    SignIn(SignInResponse),
    SignOut(SignOutResponse),
    ImageResponse(ImageResponse),
    HandShake(HandShakeResponse),
    DOS(DOSResponse),
    Error {message: String},
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SignUpResponse {
    pub client_id: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SignInResponse {
    pub success: bool
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SignOutResponse {
    pub success: bool
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ImageResponse {
    pub request_id: String,
    pub image_name: String, 
    pub encrypted_image_data: Vec<u8>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct DOSResponse {
    pub online_clients: Vec<OnlineClient>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct HandShakeResponse {
    pub success: bool
}

// HashMap to store timestamps for each request using Tokio's Mutex
static REQUEST_TIMESTAMPS: Lazy<Mutex<HashMap<String, Instant>>> = Lazy::new(|| {
    Mutex::new(HashMap::new())
});

#[derive(Clone, Serialize, Deserialize)]
pub enum RequestType {
    ImageRequest,
    ExtraViewsRequest,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ClientToClientRequest {
    pub request_type: RequestType,
    pub requested_views: u32,
    pub image_id: Option<String>, // Added image_id for extra views request
}

#[derive(Serialize, Deserialize)]
pub struct ClientToClientResponse {
    pub image_data: Vec<u8>,
    pub shared_by_ip: String,
    pub image_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct ExtraViewsResponse {
    pub image_id: String,
    pub new_allowed_views: u32,
}

#[derive(Clone)]
pub struct SharedImageInfo {
    pub image_id: String,
    pub image_path: String,
    pub shared_by: String,
}
pub struct PendingRequest {
    pub request: ClientToClientRequest,
    pub requester_ip: String,
    pub socket: TcpStream,
}

static PENDING_REQUESTS: Lazy<Arc<Mutex<Vec<PendingRequest>>>> = Lazy::new(|| {
    Arc::new(Mutex::new(Vec::new()))
});

// add a signed_up and signed_in global flags
static SIGNED_UP: Lazy<Mutex<bool>> = Lazy::new(|| {
    Mutex::new(false)
});
static SIGNED_IN: Lazy<Mutex<bool>> = Lazy::new(|| {
    Mutex::new(false)
});

static CLIENT_ID: Lazy<Mutex<String>> = Lazy::new(|| {
    Mutex::new("".to_string())
});
// Helper function to write a length-prefixed message
async fn write_length_prefixed_message(stream: &mut TcpStream, data: &[u8], close_connection: bool) -> tokio::io::Result<()> {
    // Write the length as a 4-byte unsigned integer in big-endian
    let length = data.len() as u32;
    let length_bytes = length.to_be_bytes();
    stream.write_all(&length_bytes).await?;
    stream.write_all(data).await?;
    if close_connection {
        if let Err(e) = stream.shutdown().await {
            eprintln!("Failed to shutdown socket after writing: {}", e);
        }
    }
    Ok(())
}

// Helper function to read a length-prefixed message
async fn read_length_prefixed_message(stream: &mut TcpStream, close_connection: bool) -> tokio::io::Result<Vec<u8>> {
    // Read the length as a 4-byte unsigned integer (big-endian)
    let mut length_bytes = [0u8; 4];
    stream.read_exact(&mut length_bytes).await?;
    let length = u32::from_be_bytes(length_bytes);
    // Limit the length to prevent DoS attacks
    let mut buffer = vec![0u8; length as usize];
    stream.read_exact(&mut buffer).await?;
    // close the socket
    if close_connection {
        if let Err(e) = stream.shutdown().await {
            eprintln!("Failed to shutdown socket after reading: {}", e);
        }
    }
    Ok(buffer)
}

pub async fn run_client(
    tx: mpsc::Sender<Request>,
    mut rx: mpsc::Receiver<Response>,
    client_ip: String,
) {
    let rx = Arc::new(Mutex::new(rx));
    loop {
        println!("Please choose an option:");
        println!("1. Sign up");
        println!("2. Sign in");
        println!("3. Sign Out");
        println!("4. Request DOS, list all online peers with their IP and images");
        println!("5. Encrypt an image from the server");
        println!("6. Request an image from another client");
        println!("7. Edit access rights of a client");
        println!("8. View shared images");
        println!("9. View pending requests");
        println!("10. Request Extra Views");

        let mut choice = String::new();
        io::stdin()
            .read_line(&mut choice)
            .expect("Failed to read input");
        let choice = choice.trim();
        match choice {
            "1" => {
                if *SIGNED_UP.lock().await {
                    println!("Client already signed up.");
                }else{
                    sign_up(tx.clone(), client_ip.clone()).await;
                    let mut rx_lock = rx.lock().await;
                    receive_response_from_middleware(&mut rx_lock).await;
                }
            }
            "2" => {
                if *SIGNED_IN.lock().await {
                    println!("Client already signed in.");
                }else{
                    println!("Please enter your client ID. If you don't have one, sign up first.");
                    let mut client_id = String::new();
                    io::stdin()
                        .read_line(&mut client_id)
                        .expect("Failed to read input");
                    client_id = client_id.trim().to_string();
                    let client_dir = format!("Client_{}", client_id);
                    if !does_dir_exist(&client_dir).await {
                        println!("Client ID not found. Please sign up first or bring your client directory to this machine."); 
                        continue;
                    }
                    *CLIENT_ID.lock().await = client_id.clone();
                    sign_in(tx.clone(), client_id.clone(), client_ip.clone(), true).await;   
                    let mut rx_lock = rx.lock().await;
                    receive_response_from_middleware(&mut rx_lock).await;
            }
            }
            "3" => {// Sign out
                if !*SIGNED_IN.lock().await {
                    println!("Client already signed out.");
                    continue;
                }else{
                    sign_out(tx.clone(), CLIENT_ID.lock().await.clone()).await;
                    let mut rx_lock = rx.lock().await;
                    receive_response_from_middleware(&mut rx_lock).await;
                }
            }
            "4" => { // Request DOS
                if !*SIGNED_IN.lock().await {
                    println!("Please sign in first.");
                    continue;
                }
                request_dos(tx.clone(), CLIENT_ID.lock().await.clone()).await;
                let mut rx_lock = rx.lock().await;
                receive_response_from_middleware(&mut rx_lock).await;
            }
            "5" => {// Encrypt an image from the server
                if !*SIGNED_IN.lock().await {
                    println!("Please sign in first.");
                    continue;
                }
             //   println!("Here1");
                let client_id = CLIENT_ID.lock().await.clone();
               // println!("Here2 {}", client_id);
                encrypt_image_from_server(tx.clone(), client_id).await;
                println!("Here2");
                let rx_clone = Arc::clone(&rx);
                tokio::spawn(async move {
                        let mut rx_lock = rx_clone.lock().await; // Lock the Mutex to access the receiver
        		receive_response_from_middleware(&mut rx_lock).await;
        		println!("Finished encrypting image!");
    		});
            }
            "6" => {// Request an image from another client
                if !*SIGNED_IN.lock().await {
                    println!("Please sign in first.");
                    continue;
                }
                request_image_from_client().await;
                sign_in(tx.clone(), CLIENT_ID.lock().await.clone(), client_ip.clone(), false).await;
            }
            "7" => { // Placeholder for Edit access rights of a client
                if !*SIGNED_IN.lock().await {
                    println!("Please sign in first.");
                    continue;
                }
                println!("Edit access rights functionality not yet implemented.");
            }
            "8" => {// View shared images
                if !*SIGNED_IN.lock().await {
                    println!("Please sign in first.");
                    continue;
                }
                view_shared_images().await;
            }
            "9" => {// View pending requests
                if !*SIGNED_IN.lock().await {
                    println!("Please sign in first.");
                    continue;
                }
                handle_pending_requests(client_ip.clone()).await;
            }
            "10" => { // Request Extra Views
                if !*SIGNED_IN.lock().await {
                    println!("Please sign in first.");
                    continue;
                }
            	let image_info = choose_image().await;
            	request_extra_views(image_info.expect("REASON").clone()).await;
            }
            _ => {
                println!("Invalid choice.");
            }
        }
        // dummy way to solve the communication issue
        for _ in 0..1 {
            sign_in(tx.clone(), "dummy".to_string(), client_ip.clone(), false).await;
            let shared_images_file = format!("Client_{}/shared_with_me/shared_images.txt", *CLIENT_ID.lock().await);
            let _file = match File::open(shared_images_file).await {
                Ok(file) => file,
                Err(_) => {
                    continue;
                } };
        }
        //        sign_in(tx.clone(), "dummy".to_string(), client_ip.clone(), false).await;
        
     //   hand_shake(tx.clone(), client_ip.clone()).await;
    }
}

async fn sign_up(
    tx:  mpsc::Sender<Request>,
    client_ip: String,
) {
    let request = Request::SignUp(SignUpRequest{
        client_ip: client_ip.clone(),
    });
    send_request_to_middleware(tx, request).await;
}

async fn sign_in(
    tx:  mpsc::Sender<Request>,
    client_id: String,
    client_ip: String,
    reply_back: bool,
) {
    let request = Request::SignIn(SignInRequest{
        client_id: client_id.clone(),
        client_ip: client_ip.clone(),
        reply_back,
    });
    send_request_to_middleware(tx, request).await;
}

async fn sign_out(
    tx:  mpsc::Sender<Request>,
    client_id: String,
) {
    let request = Request::SignOut(SignOutRequest{
        client_id: client_id.clone(),
    });
    send_request_to_middleware(tx, request).await;
}

async fn request_dos(
    tx:  mpsc::Sender<Request>,
    client_id: String,
) {
    let request = Request::DOS(DOSRequest{
        client_id: client_id.clone(),
    });
    send_request_to_middleware(tx, request).await;
}

async fn hand_shake(
    tx:  mpsc::Sender<Request>,
    client_ip: String,
) {
    let request = Request::HandShake(HandShakeRequest{
        client_ip: client_ip.clone(),
    });
    send_request_to_middleware(tx, request).await;
}

async fn send_request_to_middleware(
    tx:  mpsc::Sender<Request>,
    request: Request,
) {
    // Send the request to the middleware via tx
    if let Err(e) = tx.send(request).await {
        eprintln!(
            "Client: Failed to send request to middleware: {}",
            e
        );
    } 
    // else {
    //     // Check if the type of request is SignIn, don't print
    //     println!(
    //         "Request sent to middleware",
    //     );
    // }
}
async fn read_id_from_file(file_path: &str) -> io::Result<String> {
    // Read the file's content into a String
    let content = fs::read_to_string(file_path).await?;
    Ok(content.trim().to_string()) // Trim to remove any extra whitespace or newline
}

async fn receive_response_from_middleware(
    rx: &mut mpsc::Receiver<Response>,
) {
    // Receive response from the middleware via rx
    if let Some(response) = rx.recv().await {
        match response {
            Response::SignUp(res) => {
                handle_sign_up_response(res).await;
            },
            // commented as we don't receive in sign in (for the null check)
            Response::SignIn(res) => {
                if res.success {
                    *SIGNED_UP.lock().await = true;
                    *SIGNED_IN.lock().await = true;
                    println!("Client signed in successfully");
                }else{
                    println!("Client sign in failed. Attempt again");
                }
            },
            Response::SignOut(res) => {
                if res.success {
                    *SIGNED_UP.lock().await = false; // we can sign a new user up after signing out.
                    *SIGNED_IN.lock().await = false;
                    *CLIENT_ID.lock().await = "".to_string();
                    println!("Client signed out successfully");
                }else{
                    println!("Client sign out failed. Attempt again");
                }
            },
            Response::DOS(res) => {
                if res.online_clients.is_empty() {
                    println!("No online clients found.");
                } else {
                    println!("Online clients:");
                    for client in res.online_clients {
                        println!("Client ID: {}, Client IP: {}, Images: {:?}", client.client_id, client.client_ip, client.images);
                }
            }
            },
            Response::ImageResponse(res) => {
                handle_image_response(res).await;
            },
            _ => println!("Unexpected response Client."),
        }
    } else {
        eprintln!("Did not receive a response from the middleware.");
    }
}

async fn handle_sign_up_response(response: SignUpResponse) {
    let client_dir = format!("Client_{}", response.client_id);
    println!("Client registered with ID: {} and a new directory: {} is created.", response.client_id, client_dir);
    println!("Plase, remmeber your client ID to be able to sign in again.");
    tokio::fs::create_dir_all(client_dir.clone()).await.expect("Failed to create directory");
    let image_dir = format!("{}/my_images", client_dir.clone());
    tokio::fs::create_dir_all(image_dir)
    .await
    .expect("Failed to create 'my_images' directory");
    println!("Please put your images in the 'my_images' directory inslide {} directory.", client_dir);
    *SIGNED_UP.lock().await = true;
    *SIGNED_IN.lock().await = true; // we can sign the user in after signing up
    *CLIENT_ID.lock().await = response.client_id;
    // // Call the append_to_file function
    // if let Err(e) = append_to_file("client_id.txt", &response.client_id).await {
    //     eprintln!("Failed to write to file: {}", e);
    // }
}

async fn does_dir_exist(dir_path: &str) -> bool {
    match fs::metadata(dir_path).await {
        Ok(metadata) => metadata.is_dir(),
        Err(_) => false,
    }
}

async fn append_to_file(file_path: &str, content: &str) -> tokio::io::Result<()> {
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(file_path)
        .await?;

    file.write_all(format!("{}\n", content).as_bytes()).await?;
    Ok(())
}
async fn encrypt_image_from_server(
    tx: mpsc::Sender<Request>,
    client_id: String,
) {
    let image_dir = format!("Client_{}/my_images", *CLIENT_ID.lock().await);
    println!("Please enter the image file name (without .png) to encrypt (from {} folder):", image_dir);
    let mut image_file_name = String::new();
    io::stdin()
        .read_line(&mut image_file_name)
        .expect("Failed to read input");
    let image_file_name = image_file_name.trim();

    // Build the image path
    let image_path = format!("{}/{}.png", image_dir, image_file_name);

    // Read the image data
    let image_data = match fs::read(&image_path).await {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Failed to read image file {}: {}", image_path, e);
            return;
        }
    };

    let request_id = Uuid::new_v4().to_string();
    let client_id_clone = client_id.clone();

    let request = ImageRequest {
        client_id: client_id_clone.clone(),
        request_id: request_id.clone(),
        image_name: image_file_name.to_string(),
        image_data,
    };

    // Record the timestamp of the request
    {
        let mut timestamps = REQUEST_TIMESTAMPS.lock().await;
        timestamps.insert(request_id.clone(), Instant::now());
    }

    // Send the image request
    if let Err(e) = tx.send(Request::ImageRequest(request)).await {
        eprintln!(
            "Client: Failed to send image to middleware (Request ID: {}): {}",
            request_id, e
        );
        return;
    } else {
        println!(
            "Client {}: Image data with request ID {} sent to middleware.",
            client_id_clone, request_id
        );
    }
}

// handle image response from server
async fn handle_image_response(response: ImageResponse) {
    // Create the "Encrypted_images" directory if it doesn't exist
    let encrypted_images_dir = format!("Client_{}/Encrypted_images", *CLIENT_ID.lock().await);
    tokio::fs::create_dir_all(encrypted_images_dir.clone())
        .await
        .expect("Failed to create 'Encrypted_images' directory");

    // Use client_ip to construct the file name
    let encrypted_image_path = format!(
        "{}/{}_encrypted.png",
        encrypted_images_dir, response.image_name
    );
    if let Err(e) = fs::write(&encrypted_image_path, &response.encrypted_image_data).await {
        eprintln!(
            "Failed to save encrypted image {}: {}",
            &response.request_id, e
        );
    } else {
        println!(
            "Client: image {} encrypted and saved to {}",
            response.image_name, encrypted_image_path
        );
    }

    // Calculate the round trip time
    if let Some(sent_time) = {
        let mut timestamps = REQUEST_TIMESTAMPS.lock().await;
        timestamps.remove(&response.request_id)
    } {
        let round_trip_time = sent_time.elapsed();
        println!(
            "Round trip time for request {}: {:?}",
            response.request_id, round_trip_time
        );

        // Log the round trip time to the file
        let log_entry = format!(
            "Request ID: {}, Round Trip Time: {:?}\n",
            response.request_id, round_trip_time
        );

        // Write to log file
        let log_file_path = format!("Client_{}/roundtrip_times.txt", *CLIENT_ID.lock().await);
        let mut log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file_path)
            .await
            .expect("Failed to open log file");

        if let Err(e) = log_file.write_all(log_entry.as_bytes()).await {
            eprintln!("Failed to write to log file: {}", e);
        }
    } else {
        eprintln!(
            "No timestamp found for request ID {}. Unable to calculate round trip time.",
            response.request_id
        );
    }
}

pub async fn request_image_from_client() {
    println!("Enter the other client's IP address and port (format x.x.x.x:port):");
    let mut address = String::new();
    io::stdin()
        .read_line(&mut address)
        .expect("Failed to read input");
    let address = address.trim().to_string();

    // Ask for the name of the image
    println!("Enter the name of the image you want to request (without .png):");
    let mut image_name = String::new();
    io::stdin()
        .read_line(&mut image_name)
        .expect("Failed to read input");
    let image_name = image_name.trim().to_string();

    println!("Enter the number of times you want to view the image:");
    let mut views_input = String::new();
    io::stdin()
        .read_line(&mut views_input)
        .expect("Failed to read input");
    let views: u32 = views_input.trim().parse().expect("Please enter a valid number");

    // Create a request message with the image name
    let image_name_temp = image_name.clone();
    let image_request = ClientToClientRequest {
        request_type: RequestType::ImageRequest,
        requested_views: views,
        image_id: Some(image_name_temp),  // Send the image name as the image_id
    };

    // Serialize the request message
    let serialized_request =
        bincode::serialize(&image_request).expect("Failed to serialize request");

    // Connect to the other client
    match TcpStream::connect(&address).await {
        Ok(mut stream) => {
            // Send the request using length-prefixed protocol
            if let Err(e) = write_length_prefixed_message(&mut stream, &serialized_request, false).await {
                eprintln!("Failed to send request to other client: {}", e);
                return;
            }
            println!("Image Request sent to other client at {}.", address);

            // Spawn a background task to receive the image
            let image_name_clone = image_name.clone();
            tokio::spawn(async move {
                match read_length_prefixed_message(&mut stream, true).await {
                Ok(buffer) => {
                    // Deserialize the response
                    if let Ok(response) = bincode::deserialize::<ClientToClientResponse>(&buffer) {
                        handle_client_image_response(response).await;
                    } else {
                        eprintln!("Failed to deserialize response from other client.");
                    }
                }
                Err(e) => {
                    eprintln!("Failed to receive response from other client: {}", e);
                    return;
                }
            }
            });
        }
        Err(e) => {
            eprintln!("Failed to connect to other client at {}: {}", address, e);
        }
    }
}

async fn handle_client_image_response(response: ClientToClientResponse) {
    // Create the "shared_with_me" directory if it doesn't exist
    let shared_dir = format!("Client_{}/shared_with_me", *CLIENT_ID.lock().await);
    fs::create_dir_all(shared_dir.clone())
        .await
        .expect("Failed to create 'shared_with_me' directory");

    // Use the image_id to generate the image filename
    let image_path = format!("{}/{}_from_{}.png", shared_dir, response.image_id, response.shared_by_ip);

    // Save the encrypted image
    fs::write(&image_path, &response.image_data)
        .await
        .expect("Failed to save image");

    println!("Received image from {}.", response.shared_by_ip);

    // Save the image info
    let shared_images_file = format!("{}/shared_images.txt", shared_dir);
    let log_entry = format!(
        "Image ID: {}, Image Path: {}, Shared By: {}\n",
        response.image_id, image_path, response.shared_by_ip
    );

    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(shared_images_file)
        .await
        .expect("Failed to open shared_images.txt");

    if let Err(e) = log_file.write_all(log_entry.as_bytes()).await {
        eprintln!("Failed to write to shared_images.txt: {}", e);
    }

    println!("Image received and saved to {}.", image_path);
}

async fn choose_image() -> Result<SharedImageInfo, Box<dyn std::error::Error>> {
    use tokio::fs::File;
    use tokio::io::{AsyncBufReadExt, BufReader};

    let shared_images_file = format!("Client_{}/shared_with_me/shared_images.txt", *CLIENT_ID.lock().await);

    let file = File::open(shared_images_file).await?;

    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let mut shared_images = Vec::new();

    while let Some(line) = lines.next_line().await? {
        // Parse the line
        if let Some(info) = parse_shared_image_line(&line) {
            shared_images.push(info);
        }
    }

    if shared_images.is_empty() {
        let err_msg = "No shared images available.";
        println!("{}", err_msg);
        return Err(err_msg.into());
    }

    // Display the list of shared images
    println!("Shared images:");
    for (index, image_info) in shared_images.iter().enumerate() {
        println!(
            "{}. Image ID: {}, Shared By: {}",
            index + 1,
            image_info.image_id,
            image_info.shared_by,
        );
    }

    // Prompt the user to select an image
    println!("Enter the number of the image you want to view (or 'q' to quit):");
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read input");
    let input = input.trim();

    if input.eq_ignore_ascii_case("q") {
        return Err("User chose to quit.".into());
    }

    let choice: usize = match input.parse() {
        Ok(num) => num,
        Err(_) => {
            let err_msg = "Invalid choice.";
            println!("{}", err_msg);
            return Err(err_msg.into());
        }
    };

    if choice == 0 || choice > shared_images.len() {
        let err_msg = "Invalid choice.";
        println!("{}", err_msg);
        return Err(err_msg.into());
    }

    // Get the selected image info
    let image_info = shared_images[choice - 1].clone();

    Ok(image_info)
}


async fn view_shared_images() {


    let shared_images_file = format!("Client_{}/shared_with_me/shared_images.txt", *CLIENT_ID.lock().await);
    let file = match File::open(shared_images_file).await {
        Ok(file) => file,
        Err(_) => {
            println!("No shared images found.");
            return;
        }
    };

    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let mut shared_images = Vec::new();

    while let Some(line) = lines.next_line().await.unwrap_or(None) {
        // Parse the line
        if let Some(info) = parse_shared_image_line(&line) {
            shared_images.push(info);
        }
    }

    if shared_images.is_empty() {
        println!("No shared images available.");
        return;
    }

    // Display the list of shared images
    println!("Shared images:");
    for (index, image_info) in shared_images.iter().enumerate() {
        println!(
            "{}. Image ID: {}, Shared By: {}",
            index + 1,
            image_info.image_id,
            image_info.shared_by,
            //allowed_views
        );
    }

    // Prompt the user to select an image
    println!("Enter the number of the image you want to view (or 'q' to quit):");
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read input");
    let input = input.trim();

    if input.eq_ignore_ascii_case("q") {
        return;
    }

    let choice: usize = match input.parse() {
        Ok(num) => num,
        Err(_) => {
            println!("Invalid choice.");
            return;
        }
    };

    if choice == 0 || choice > shared_images.len() {
        println!("Invalid choice.");
        return;
    }

    // Get the selected image info
    let image_info = shared_images[choice - 1].clone();

    // Load the encrypted image
    let mut encrypted_image = match image::open(&image_info.image_path) {
        Ok(img) => img,
        Err(e) => {
            eprintln!("Failed to open image file: {}", e);
            return;
        }
    };

    // Extract allowed_views from the image
    let allowed_views = match extract_allowed_views_from_image(&encrypted_image) {
        Ok(views) => views,
        Err(e) => {
            eprintln!("Failed to extract allowed views: {}", e);
            return;
        }
    };

    if allowed_views == 0 {
        println!("No more allowed views for this image.");
        // Prompt the user to request extra views
        println!("Would you like to request extra views? (y/n)");
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .expect("Failed to read input");
        let input = input.trim();

        if input.eq_ignore_ascii_case("y") {
            // Request extra views
            request_extra_views(image_info.clone()).await;
        }
        return;
    }

    // Decrypt the image and save it to a temporary file
    let decoded_image_buffer = extract_hidden_image_buffer_from_encoded(encrypted_image.clone());

    // Save the decrypted image to a temporary file
    let decrypted_image_path = "decrypted_image.png"; 
    fs::write(decrypted_image_path, &decoded_image_buffer).await.expect("Failed to save decrypted image");

    // Open the image using an external viewer
    if let Err(e) = open_image(decrypted_image_path).await {
        eprintln!("Failed to open image: {}", e);
    }

    // Delete the decrypted image file
    if let Err(e) = fs::remove_file(decrypted_image_path).await {
        eprintln!("Failed to delete decrypted image file: {}", e);
    }

    // Decrement allowed_views
    let new_allowed_views = allowed_views - 1;
    println!("Remaining allowed views: {}", new_allowed_views);

    // Embed the new allowed_views into the encrypted image
    embed_allowed_views_in_image(&mut encrypted_image, new_allowed_views).unwrap();

    // Save the updated encrypted image back to the file
    encrypted_image.save(&image_info.image_path).unwrap();
}

async fn request_extra_views(image_info: SharedImageInfo) {
    println!("Enter the number of extra views you would like to request:");
    let mut views_input = String::new();
    io::stdin()
        .read_line(&mut views_input)
        .expect("Failed to read input");
    let requested_views: u32 = match views_input.trim().parse() {
        Ok(num) => num,
        Err(_) => {
            println!("Invalid number.");
            return;
        }
    };

    // Build the request
    let extra_views_request = ClientToClientRequest {
        request_type: RequestType::ExtraViewsRequest,
        requested_views,
        image_id: Some(image_info.image_id.clone()),
    };

    // Serialize the request
    let serialized_request = match bincode::serialize(&extra_views_request) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Failed to serialize extra views request: {}", e);
            return;
        }
    };

    let image_path = image_info.image_path.clone();
    let image_id = image_info.image_id.clone();
    let shared_by = image_info.shared_by.clone();

    // Spawn a background task to handle the response
    task::spawn(async move {
        // Connect to the owner
        match TcpStream::connect(&shared_by).await {
            Ok(mut stream) => {
                // Send the request using length-prefixed protocol
                if let Err(e) = write_length_prefixed_message(&mut stream, &serialized_request, false).await {
                    eprintln!("Failed to send extra views request to owner: {}", e);
                    return;
                }

                // Flush the stream to ensure data is sent
                if let Err(e) = stream.flush().await {
                    eprintln!("Failed to flush stream: {}", e);
                }

                // Receive the response
                match read_length_prefixed_message(&mut stream, true).await {
                    Ok(buffer) => {
                        // Deserialize the response
                        if let Ok(response) = bincode::deserialize::<ExtraViewsResponse>(&buffer) {
                            if response.image_id != image_id {
                                eprintln!("Image ID mismatch in extra views response.");
                                return;
                            }

                            // Update the image with the new allowed views
                            if let Err(e) = update_image_allowed_views(&image_path, response.new_allowed_views) {
                                eprintln!("Failed to update image with new allowed views: {}", e);
                            } else {
                                println!("Received {} extra views for image ID {}.", response.new_allowed_views, image_id);
                            }
                        } else {
                            eprintln!("Failed to deserialize extra views response from owner.");
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to receive response from owner: {}", e);
                        return;
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to connect to owner at {}: {}", shared_by, e);
            }
        }
    });

    // Immediately return to the menu
    println!("Extra views request sent. You will be notified when the extra views are received.");
}


fn update_image_allowed_views(image_path: &str, new_allowed_views: u32) -> Result<(), Box<dyn std::error::Error>> {
    // Load the encrypted image
    let mut encrypted_image = image::open(image_path)?;

    // Extract the current allowed views from the image
    let current_allowed_views = extract_allowed_views_from_image(&encrypted_image)?;

    // Add the new allowed views to the current allowed views
    let total_allowed_views = current_allowed_views.checked_add(new_allowed_views)
        .ok_or("Integer overflow when adding allowed views")?;

    // Embed the total allowed views back into the image
    embed_allowed_views_in_image(&mut encrypted_image, total_allowed_views)?;

    // Save the updated encrypted image back to the file
    encrypted_image.save(image_path)?;

    Ok(())
}


// Function to parse a line from shared_images.txt into SharedImageInfo
fn parse_shared_image_line(line: &str) -> Option<SharedImageInfo> {
    let parts: Vec<&str> = line.split(", ").collect();
    if parts.len() != 3 {
        return None;
    }

    let image_id_part = parts[0];
    let image_path_part = parts[1];
    let shared_by_part = parts[2];

    let image_id = image_id_part.strip_prefix("Image ID: ")?;
    let image_path = image_path_part.strip_prefix("Image Path: ")?;
    let shared_by = shared_by_part.strip_prefix("Shared By: ")?;

    Some(SharedImageInfo {
        image_id: image_id.to_string(),
        image_path: image_path.to_string(),
        shared_by: shared_by.to_string(),
    })
}


// Implement the function to open the image (decrypted image)
async fn open_image(image_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::process::Command;

    // Display the image using an external viewer
    let mut child = Command::new("eog")
        .arg(image_path)
        .spawn()
        .expect("Failed to open image viewer");

    child.wait().await?;

    Ok(())
}

// Decryption function provided
fn extract_hidden_image_buffer_from_encoded(encoded_img: DynamicImage) -> Vec<u8> {
    let encoded_rgba_img = encoded_img.to_rgba();  // Convert encoded image to RGBA format
    let decoder = Decoder::new(encoded_rgba_img);
    decoder.decode_alpha()
}

// Function to embed allowed_views into the encrypted image
fn embed_allowed_views_in_image(image: &mut DynamicImage, allowed_views: u32) -> Result<(), Box<dyn std::error::Error>> {
    let mut rgba_image = image.to_rgba();
    let (width, height) = rgba_image.dimensions();

    // Coordinates for the lower-left corner
    let x = 0;
    let y = height - 1;

    // Separator (magic number)
    let separator: u32 = 0xDEADBEEF;
    let separator_bytes = separator.to_be_bytes(); // Big-endian byte order
    let allowed_views_bytes = allowed_views.to_be_bytes();

    // Embed the separator and allowed_views into the alpha channel
    let mut data_bytes = separator_bytes.to_vec();
    data_bytes.extend_from_slice(&allowed_views_bytes);

    // Check if we have enough pixels to embed the data
    if (x as usize + data_bytes.len()) > width as usize {
        return Err("Not enough space to embed data".into());
    }

    // Embed data into the alpha channel starting from (x, y)
    for (i, &byte) in data_bytes.iter().enumerate() {
        let xi = x + i as u32;
        let pixel = rgba_image.get_pixel_mut(xi, y);
        pixel[3] = byte; // Modify alpha channel directly
    }

    *image = DynamicImage::ImageRgba8(rgba_image);
    Ok(())
}

// Function to extract allowed_views from the encrypted image
fn extract_allowed_views_from_image(image: &DynamicImage) -> Result<u32, Box<dyn std::error::Error>> {
    let rgba_image = image.to_rgba();
    let (width, height) = rgba_image.dimensions();

    let x = 0;
    let y = height - 1;

    // Read the separator and allowed_views bytes from the alpha channel
    let mut data_bytes = Vec::new();

    // Read 8 bytes (4 bytes for separator, 4 bytes for allowed_views)
    for i in 0..8 {
        let xi = x + i as u32;
        if xi >= width {
            return Err("Image too small to extract data".into());
        }
        let pixel = rgba_image.get_pixel(xi, y);
        let byte = pixel[3]; // Alpha channel
        data_bytes.push(byte);
    }

    // Extract separator and allowed_views
    let separator_bytes = &data_bytes[0..4];
    let allowed_views_bytes = &data_bytes[4..8];

    let separator = u32::from_be_bytes([
        separator_bytes[0],
        separator_bytes[1],
        separator_bytes[2],
        separator_bytes[3],
    ]);
    if separator != 0xDEADBEEF {
        return Err("Invalid separator".into());
    }

    let allowed_views = u32::from_be_bytes([
        allowed_views_bytes[0],
        allowed_views_bytes[1],
        allowed_views_bytes[2],
        allowed_views_bytes[3],
    ]);

    Ok(allowed_views)
}

// Implement the client server
pub async fn run_client_server(client_ip: String) {
    let listener = TcpListener::bind(client_ip.clone())
        .await
        .expect("Failed to bind to port");
    println!("Client server running on {}", client_ip);

    loop {
        match listener.accept().await {
            Ok((mut socket, addr)) => {
                println!("Received a connection from {}", addr);

                tokio::spawn(async move {
                    // Read the request
                    match read_length_prefixed_message(&mut socket, false).await {
                        Ok(buffer) => {
                            // Handle the request
                            handle_client_request(buffer, socket).await;
                        }
                        Err(e) => {
                            eprintln!("Failed to read from socket: {}", e);
                            return;
                        }
                    }
                });
            }
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
            }
        }
    }
}

async fn handle_client_request(
    buffer: Vec<u8>,
    socket: TcpStream,
) {
    let requester_ip = socket.peer_addr().unwrap().to_string();

    // Deserialize the request
    if let Ok(request) = bincode::deserialize::<ClientToClientRequest>(&buffer) {
        let pending_request = PendingRequest {
            request,
            requester_ip: requester_ip.clone(),
            socket,
        };

        // Add to PENDING_REQUESTS
        let mut pending_requests = PENDING_REQUESTS.lock().await;
        pending_requests.push(pending_request);
        println!("Added a new pending request from {}", requester_ip);
    } else {
        eprintln!("Failed to deserialize client request.");
    }
}

async fn handle_pending_requests(client_ip: String) {
    loop {
        let pending_request = {
            let mut pending_requests_lock = PENDING_REQUESTS.lock().await;
            if pending_requests_lock.is_empty() {
                println!("No pending requests.");
                return;
            }
            // Remove the first pending request
            pending_requests_lock.remove(0)
        };

        let requester_ip = pending_request.requester_ip.clone();
        let request = pending_request.request.clone();

        match request.request_type {
            RequestType::ImageRequest => {
                println!(
                    "Image request from {}: {} views requested for image {}.",
                    requester_ip,
                    request.requested_views,
                    request.image_id.clone().unwrap_or_else(|| "unknown".to_string())
                );

                // Ensure we have an image name in the request
                let image_name = request.image_id.clone().unwrap_or_else(|| "default_image".to_string());

                println!("Approve image request? (y/n)");
                let mut approval = String::new();
                io::stdin()
                    .read_line(&mut approval)
                    .expect("Failed to read input");
                let approval = approval.trim();

                if approval.to_lowercase() == "y" {
                    // Optionally adjust the number of views
                    println!(
                        "Enter the number of views you want to allow (press Enter to keep it at {}):",
                        request.requested_views
                    );
                    let mut views_input = String::new();
                    io::stdin()
                        .read_line(&mut views_input)
                        .expect("Failed to read input");
                    let allowed_views = if views_input.trim().is_empty() {
                        request.requested_views
                    } else {
                        views_input
                            .trim()
                            .parse()
                            .expect("Please enter a valid number")
                    };

                    // Send the response
                    if let Err(e) = send_response_to_requester(pending_request, allowed_views, client_ip.clone(), image_name).await {
                        eprintln!("Failed to send response: {}", e);
                    } else {
                        println!("Response sent.");
                    }
                } else {
                    // Deny the request
                    if let Err(e) = send_denial_to_requester(pending_request.socket).await {
                        eprintln!("Failed to send denial: {}", e);
                    } else {
                        println!("Denial sent.");
                    }
                }
            }
            RequestType::ExtraViewsRequest => {
                let image_id = request.image_id.unwrap_or_else(|| "".to_string());
                println!(
                    "Extra views request from {}: {} views requested for image ID {}.",
                    requester_ip,
                    request.requested_views,
                    image_id
                );

                println!("Approve extra views request? (y/n)");
                let mut approval = String::new();
                io::stdin()
                    .read_line(&mut approval)
                    .expect("Failed to read input");
                let approval = approval.trim();

                if approval.to_lowercase() == "y" {
                    // Optionally adjust the number of views
                    println!(
                        "Enter the number of extra views you want to allow (press Enter to keep it at {}):",
                        request.requested_views
                    );
                    let mut views_input = String::new();
                    io::stdin()
                        .read_line(&mut views_input)
                        .expect("Failed to read input");
                    let allowed_views = if views_input.trim().is_empty() {
                        request.requested_views
                    } else {
                        views_input
                            .trim()
                            .parse()
                            .expect("Please enter a valid number")
                    };

                    // Send the response
                    if let Err(e) = send_extra_views_response(pending_request.socket, image_id.clone(), allowed_views).await {
                        eprintln!("Failed to send extra views response: {}", e);
                    } else {
                        println!("Extra views response sent.");
                    }
                } else {
                    // Deny the request
                    if let Err(e) = send_denial_to_requester(pending_request.socket).await {
                        eprintln!("Failed to send denial: {}", e);
                    } else {
                        println!("Denial sent.");
                    }
                }
            }
        }
    }
}

async fn send_response_to_requester(
    pending_request: PendingRequest,
    allowed_views: u32,
    client_ip: String,
    image_name: String,
) -> Result<(), Box<dyn std::error::Error>> {
    // Construct the image file path based on the requested image name
    let image_file_path = format!("Client_{}/Encrypted_images/{}_encrypted.png", *CLIENT_ID.lock().await, image_name.clone());

    // Read the encrypted image file
    let mut image = match image::open(&image_file_path) {
        Ok(img) => img,
        Err(e) => {
            eprintln!("Failed to open image file {}: {}", image_file_path, e);
            return Err(Box::new(e));
        }
    };

    // Embed the allowed_views into the image
    embed_allowed_views_in_image(&mut image, allowed_views)?;

    // Save the modified image to a buffer
    let mut image_buffer = Vec::new();
    image.write_to(&mut image_buffer, image::ImageOutputFormat::PNG)?;

    // Generate an image_id
    //let image_id = Uuid::new_v4().to_string();
    // Create a response struct that includes the image data and the shared_by_ip
    let response = ClientToClientResponse {
        image_data: image_buffer,
        shared_by_ip: client_ip.clone(),
        image_id: image_name.clone(),
    };

    // Serialize the response
    let serialized_response = bincode::serialize(&response)?;

    // Send the serialized response using length-prefixed protocol
    let mut socket = pending_request.socket;
    write_length_prefixed_message(&mut socket, &serialized_response, true).await?;
    socket.flush().await?;
    Ok(())
}

async fn send_extra_views_response(
    mut socket: TcpStream,
    image_id: String,
    new_allowed_views: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = ExtraViewsResponse {
        image_id,
        new_allowed_views,
    };

    // Serialize the response
    let serialized_response = bincode::serialize(&response)?;

    // Send the response using length-prefixed protocol
    write_length_prefixed_message(&mut socket, &serialized_response, true).await?;

    Ok(())
}

async fn send_denial_to_requester(mut socket: TcpStream) -> Result<(), Box<dyn std::error::Error>> {
    // Send a denial response or simply close the connection
    let denial_message = "Request denied by the user.";
    let denial_data = denial_message.as_bytes();
    write_length_prefixed_message(&mut socket, denial_data, true).await?;
    Ok(())
}

