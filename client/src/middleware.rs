use tokio::net::TcpStream;
use futures::future::join_all;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::sync::mpsc::{Receiver, Sender};
use crate::client::ImageRequest;
use bincode;
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct LightRequest {
    pub client_ip: String,
    pub request_id: String,
    pub message_data: String,
}

/// Main function to handle image data transmission through an elected server
pub async fn run_middleware(
    mut rx: Receiver<ImageRequest>, 
    tx: Sender<Vec<u8>>, 
    server_ips: Vec<String>
) {
    while let Some(image_request) = rx.recv().await {
        // Clone tx and server_ips for use in the spawned task
        let tx_clone = tx.clone();
        let server_ips_clone = server_ips.clone();
        
        // Spawn a new task to handle each request concurrently
        let client_ip = image_request.client_ip.clone();
        let request_id = image_request.request_id.clone();
        tokio::spawn(async move {
            // Attempt to connect to an available server
            if let Some(mut server_stream) = find_available_server(&server_ips_clone, &client_ip, &request_id).await {
                // Print IP of server
                println!(
                    "Client Middleware: Client Connected {} to server {} for request number {}.",
                    client_ip, server_stream.peer_addr().unwrap(), request_id
                );
                
                // Send the serialized data to the server
                if let Err(e) = server_stream.write_all(&image_request.image_data).await {
                    eprintln!("Client Middleware: Failed to send image data to server: {}", e);
                    return;
                }

                // Signal the server to start processing
                if let Err(e) = server_stream.shutdown().await {
                    eprintln!("Client Middleware: Failed to shutdown write stream: {}", e);
                    return;
                }

                // Receive the encrypted response from the server
                let mut response = Vec::new();
                if let Err(e) = server_stream.read_to_end(&mut response).await {
                    eprintln!("Client Middleware: Failed to read encrypted response from server: {}", e);
                    return;
                }

                // Send the response back to the client
                if let Err(e) = tx_clone.send(response).await {
                    eprintln!("Client Middleware: Failed to send response to client: {}", e);
                }
            } else {
                eprintln!("Client Middleware: No servers are available to handle the request.");
            }
        });
    }
}

/// Function to attempt to connect to any available server
async fn find_available_server(
    server_ips: &[String], 
    client_ip: &String, 
    request_id: &String
) -> Option<TcpStream> {
    // Create a vector of futures, one for each server
    let connection_futures: Vec<_> = server_ips
        .iter()
        .map(|ip| async move {
            // Try to connect to the server
            if let Ok(mut stream) = TcpStream::connect(ip).await {
                // Set no-delay option for the connection
                stream.set_nodelay(true).ok();

                // Serialize the LightRequest
                let light = LightRequest {
                    client_ip: client_ip.clone(),
                    request_id: request_id.clone(),
                    message_data: "I want to send".to_string(),
                };
                
                let serialized_data = match bincode::serialize(&light) {
                    Ok(data) => data,
                    Err(e) => {
                        eprintln!("Failed to serialize LightRequest {}: {}", light.request_id, e);
                        return None;
                    }
                };

                // Send the serialized data to the server
                if let Err(e) = stream.write_all(&serialized_data).await {
                    eprintln!("Client Middleware: Failed to send data to server: {}", e);
                    return None;
                }

                // Read the response from the server
                let mut buffer = [0; 1024];
                if let Ok(bytes_read) = stream.read(&mut buffer).await {
                    let response = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();
                    if !response.is_empty() {
                        println!("Client Middleware: Server at {} accepted the request", ip);
                        return Some(stream);
                    }
                }
            }
            None
        })
        .collect();

    // Execute all connection attempts concurrently
    let results = join_all(connection_futures).await;
    
    // Return the first successful connection
    results.into_iter().flatten().next()
}
