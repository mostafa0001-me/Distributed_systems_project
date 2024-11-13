// src/client.rs

use tokio::sync::mpsc::{Receiver, Sender};
use tokio::fs;
use uuid::Uuid;
use tokio::task;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::Mutex as TokioMutex;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use once_cell::sync::Lazy;
use std::io::{self};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use bincode;
use image::{DynamicImage, RgbaImage};
use steganography::decoder::Decoder;

#[derive(Clone, Serialize, Deserialize)]
pub struct ImageResponse {
    pub request_id: String,
    pub encrypted_image_data: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ImageRequest {
    pub client_ip: String,
    pub request_id: String,
    pub image_data: Vec<u8>,
}

// HashMap to store timestamps for each request using Tokio's Mutex
static REQUEST_TIMESTAMPS: Lazy<TokioMutex<HashMap<String, Instant>>> = Lazy::new(|| {
    TokioMutex::new(HashMap::new())
});

#[derive(Serialize, Deserialize)]
enum RequestType {
    ImageRequest,
}

#[derive(Serialize, Deserialize)]
struct ClientToClientRequest {
    request_type: RequestType,
    requested_views: u32,
}

#[derive(Serialize, Deserialize)]
struct ClientToClientResponse {
    image_data: Vec<u8>,
    allowed_views: u32,
}

// Helper function to write a length-prefixed message
async fn write_length_prefixed_message(stream: &mut TcpStream, data: &[u8]) -> tokio::io::Result<()> {
    // Write the length as a 4-byte unsigned integer (big-endian)
    let length = data.len() as u32;
    stream.write_u32(length).await?;
    // Write the data
    stream.write_all(data).await?;
    Ok(())
}

// Helper function to read a length-prefixed message
async fn read_length_prefixed_message(stream: &mut TcpStream) -> tokio::io::Result<Vec<u8>> {
    // Read the length as a 4-byte unsigned integer (big-endian)
    let length = stream.read_u32().await?;
    // Limit the length to prevent DoS attacks
    let mut buffer = vec![0u8; length as usize];
    stream.read_exact(&mut buffer).await?;
    Ok(buffer)
}

pub async fn run_client(
    tx: Sender<ImageRequest>,
    rx: Receiver<ImageResponse>,
    client_ip: String,
    n: u32,
) {
    println!("Please choose an option:");
    println!("1. Encrypt an image from the server");
    println!("2. Request an image from another client");

    let mut choice = String::new();
    io::stdin()
        .read_line(&mut choice)
        .expect("Failed to read input");
    let choice = choice.trim();

    match choice {
        "1" => {
            // Existing code for encrypting an image from the server
            encrypt_image_from_server(tx, rx, client_ip.clone(), n).await;
        }
        "2" => {
            // New code for requesting an image from another client
            request_image_from_client().await;
        }
        _ => {
            println!("Invalid choice.");
        }
    }
}

async fn encrypt_image_from_server(
    tx: Sender<ImageRequest>,
    mut rx: Receiver<ImageResponse>,
    client_ip: String,
    n: u32,
) {
    let image_paths = vec!["pic0.png"]; // Replace with your image paths
    let l: u32 = image_paths.len() as u32;
    let mut i: u32 = 0;

    // Initialize the log file
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("roundtrip_times.txt")
        .await
        .expect("Failed to open log file");
    
    let log_file = Arc::new(TokioMutex::new(log_file));

    while i < n {
        let image_path = image_paths[(i % l) as usize];
        let image_data = match fs::read(image_path).await {
            Ok(data) => data,
            Err(e) => {
                eprintln!("Failed to read image file {}: {}", image_path, e);
                i += 1;
                continue;
            }
        };
        let request_id = Uuid::new_v4().to_string();
        let client_ip_clone = client_ip.clone();

        let request = ImageRequest {
            client_ip: client_ip_clone.clone(),
            request_id: request_id.clone(),
            image_data,
        };

        // Record the timestamp of the request
        {
            let mut timestamps = REQUEST_TIMESTAMPS.lock().await;
            timestamps.insert(request_id.clone(), Instant::now());
        }

        // Spawn a task to send the image concurrently
        let tx_clone = tx.clone();
        let client_ip_clone = client_ip.clone();
        let request_id_clone = request_id.clone();
        task::spawn(async move {
            if let Err(e) = tx_clone.send(request).await {
                eprintln!(
                    "Client: Failed to send image to middleware (Request ID: {}): {}",
                    request_id_clone, e
                );
            } else {
                println!(
                    "Client {}: Image data with request ID {} sent to middleware.",
                    client_ip_clone, request_id_clone
                );
            }
        });

        i += 1;
    }

    // Receive responses from the middleware and calculate the round trip time
    while let Some(response) = rx.recv().await {
        // Use client_ip to construct the file name
        let encrypted_image_path = format!("encrypted_image_{}.png", client_ip.replace(":", "_"));
        if let Err(e) = fs::write(&encrypted_image_path, &response.encrypted_image_data).await {
            eprintln!(
                "Failed to save encrypted image {}: {}",
                &response.request_id, e
            );
        } else {
            println!(
                "Client: Encrypted image {} saved!",
                &response.request_id
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

            // Acquire the lock and write to the file
            let mut file = log_file.lock().await;
            if let Err(e) = file.write_all(log_entry.as_bytes()).await {
                eprintln!("Failed to write to log file: {}", e);
            }
        } else {
            eprintln!(
                "No timestamp found for request ID {}. Unable to calculate round trip time.",
                response.request_id
            );
        }
    }
}

async fn request_image_from_client() {
    println!("Enter the other client's IP address and port (format x.x.x.x:port):");
    let mut address = String::new();
    io::stdin()
        .read_line(&mut address)
        .expect("Failed to read input");
    let address = address.trim().to_string();

    println!("Enter the number of times you want to view the image:");
    let mut views_input = String::new();
    io::stdin()
        .read_line(&mut views_input)
        .expect("Failed to read input");
    let views: u32 = views_input.trim().parse().expect("Please enter a valid number");

    // Create a request message
    let image_request = ClientToClientRequest {
        request_type: RequestType::ImageRequest,
        requested_views: views,
    };

    // Serialize the request message
    let serialized_request =
        bincode::serialize(&image_request).expect("Failed to serialize request");

    // Connect to the other client
    match TcpStream::connect(&address).await {
        Ok(mut stream) => {
            // Send the request using length-prefixed protocol
            if let Err(e) = write_length_prefixed_message(&mut stream, &serialized_request).await {
                eprintln!("Failed to send request to other client: {}", e);
                return;
            }

            // Receive the response
            match read_length_prefixed_message(&mut stream).await {
                Ok(buffer) => {
                    // Deserialize the response
                    if let Ok(response) = bincode::deserialize::<ClientToClientResponse>(&buffer) {
                        handle_image_response(response).await;
                    } else {
                        eprintln!("Failed to deserialize response from other client.");
                    }
                }
                Err(e) => {
                    eprintln!("Failed to receive response from other client: {}", e);
                    return;
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to connect to other client at {}: {}", address, e);
        }
    }
}

async fn handle_image_response(response: ClientToClientResponse) {
    println!(
        "Received image with {} allowed views.",
        response.allowed_views
    );

    // Save the image data to a temporary file
    let temp_image_path = "temp_image.png";
    fs::write(temp_image_path, &response.image_data)
        .await
        .expect("Failed to save image");

    let mut views_remaining = response.allowed_views;

    while views_remaining > 0 {
        println!(
            "Press Enter to view the image ({} views remaining).",
            views_remaining
        );
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .expect("Failed to read input");

        // Decrypt and display the image
        if let Err(e) = open_image(temp_image_path).await {
            eprintln!("Failed to open image: {}", e);
        }

        views_remaining -= 1;
    }

    // Delete the temporary image file
//    fs::remove_file(temp_image_path)
//        .await
//        .expect("Failed to delete temporary image file");
}

// Implement the function to open and decrypt the image
async fn open_image(image_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use image::open as image_open;
    use tokio::process::Command;

    // Read the image file
    let encoded_img = image_open(image_path)?;

    // Decrypt the image using the provided function
    let decoded_image_buffer = extract_hidden_image_buffer_from_encoded(encoded_img);

    // Save the decrypted image to a temporary file
    let decrypted_image_path = "decrypted_image.png";
    fs::write(decrypted_image_path, &decoded_image_buffer).await?;

    // Display the image using an external viewer
    let mut child = Command::new("eog")
    .arg(decrypted_image_path)
    .spawn()
    .expect("Failed to open image viewer");


    child.wait().await?;

    // Delete the decrypted image file
    fs::remove_file(decrypted_image_path)
        .await
        .expect("Failed to delete decrypted image file");

    Ok(())
}

// Decryption function provided
fn extract_hidden_image_buffer_from_encoded(encoded_img: DynamicImage) -> Vec<u8> {
    let encoded_rgba_img: RgbaImage = encoded_img.to_rgba();  // Convert encoded image to RGBA format
    let decoder = Decoder::new(encoded_rgba_img);
    decoder.decode_alpha()
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

                let client_ip_clone = client_ip.clone();

                tokio::spawn(async move {
                    // Read the request
                    match read_length_prefixed_message(&mut socket).await {
                        Ok(buffer) => {
                            // Deserialize the request
                            if let Ok(request) = bincode::deserialize::<ClientToClientRequest>(&buffer) {
                                // Handle the request
                                handle_client_request(request, socket, client_ip_clone).await;
                            } else {
                                eprintln!("Failed to deserialize client request.");
                            }
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

async fn handle_client_request(request: ClientToClientRequest, mut socket: TcpStream, client_ip: String) {
    // Prompt the user to approve or deny the request
    println!(
        "Another client is requesting to view your image {} times.",
        request.requested_views
    );
    println!("Do you approve? (y/n)");
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

        // Construct the image file path based on your client_ip
        let image_file_path = format!("encrypted_pic_0_ec296eef-d0ef-4769-bf6b-98f1326aed53.png");

        // Read the image file
        let image_data = fs::read(&image_file_path)
            .await
            .expect(&format!("Failed to read image file {}", image_file_path));

        // Create the response
        let response = ClientToClientResponse {
            image_data,
            allowed_views,
        };

        // Serialize the response
        let serialized_response = bincode::serialize(&response).expect("Failed to serialize response");
        // Send the response using length-prefixed protocol
        if let Err(e) = write_length_prefixed_message(&mut socket, &serialized_response).await {
            eprintln!("Failed to send response to client: {}", e);
        }
    } else {
        // Send a denial response or simply close the connection
        eprintln!("Request denied by the user.");
        // Optionally, you can send a specific denial message
        let denial_message = "Request denied by the user.";
        let denial_data = denial_message.as_bytes();
        // Send the denial message using length-prefixed protocol
        if let Err(e) = write_length_prefixed_message(&mut socket, denial_data).await {
            eprintln!("Failed to send denial message to client: {}", e);
        }
    }
}

