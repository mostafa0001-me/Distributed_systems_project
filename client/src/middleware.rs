use tokio::net::TcpStream;
use futures::future::join_all;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::sync::mpsc::{Receiver, Sender};
use crate::client::ImageRequest;
use bincode;

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
        tokio::spawn(async move {
            // Attempt to connect to an available server
            if let Some(mut server_stream) = find_available_server(&server_ips_clone).await {
                // print ip of server
                println!("Client Middleware: Client Connected {} to server {} for request number {}.", image_request.client_ip, server_stream.peer_addr().unwrap(), image_request.request_id);


                // Serialize the ImageRequest
                let serialized_data = bincode::serialize(&image_request)
                    .expect(&format!("Failed to serialize ImageRequest {}", image_request.request_id));

                // Send the serialized data to the server
                if let Err(e) = server_stream.write_all(&serialized_data).await {
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


async fn find_available_server(server_ips: &[String]) -> Option<TcpStream> {
    // Create a random number generator
    let mut rng = thread_rng();

    // Choose a random server IP from the list
    if let Some(random_ip) = server_ips.choose(&mut rng) {
        println!("Client Middleware: Chosen server IP is {}", random_ip);

        // Try to connect to the chosen server
        if let Ok(mut stream) = TcpStream::connect(random_ip).await {
            // Set timeouts for the connection
            stream.set_nodelay(true).ok();

            // Send the initial message
            if let Ok(_) = stream.write_all(b"I want to send").await {
                println!("Client Middleware: Sent 'I want to send' message to server at {}", random_ip);

                // Read the response
                let mut buffer = [0; 1024];
                if let Ok(bytes_read) = stream.read(&mut buffer).await {
                    let response = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();
                    if !response.is_empty() {
                        println!("Client Middleware: Server at {} accepted the request", random_ip);
                        return Some(stream);
                    }
                }
            }
        } else {
            println!("Client Middleware: Failed to connect to server at {}", random_ip);
        }
    } else {
        println!("Client Middleware: No server IPs provided or list is empty.");
    }

    None
}

async fn find_available_server_t(server_ips: &[String]) -> Option<TcpStream> {
    // Create a vector of futures, one for each server
    let connection_futures: Vec<_> = server_ips
        .iter()
        .map(|ip| async move {
            // Try to connect to the server
            if let Ok(mut stream) = TcpStream::connect(ip).await {
                // Set timeouts for the connection
                stream.set_nodelay(true).ok();
                
                // Send the initial message
                if let Ok(_) = stream.write_all(b"I want to send").await {
                    println!("Client Middleware: Sent 'I want to send' message to server at {}", ip);

                    // Read the response
                    let mut buffer = [0; 1024];
                    if let Ok(bytes_read) = stream.read(&mut buffer).await {
                        let response = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();
                        if !response.is_empty() {
                            println!("Client Middleware: Server at {} accepted the request", ip);
                            return Some((ip.clone(), stream));
                        }
                    }
                }
            }
            None
        })
        .collect();

    // Execute all connection attempts concurrently
    let results = join_all(connection_futures).await;
    
    // Return the first successful connection
    results.into_iter().flatten().next().map(|(_, stream)| stream)
}

