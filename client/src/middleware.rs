use tokio::net::TcpStream;
use futures::future::join_all;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::sync::mpsc::{Receiver, Sender};

/// Main function to handle image data transmission through an elected server
pub async fn run_middleware(
    mut rx: Receiver<Vec<u8>>, 
    tx: Sender<Vec<u8>>, 
    server_ips: Vec<String>
) {
    // Attempt to connect and select an available server
    if let Some(mut server_stream) = find_available_server(&server_ips).await {
        println!("Client Middleware: Maintaining connection with the selected server.");

        // Receive image data from client
        if let Some(image_data) = rx.recv().await {
            println!("Client Middleware: Image data received from client.");

            // Send image data to the selected server
            if let Err(e) = server_stream.write_all(&image_data).await {
                eprintln!("Client Middleware: Failed to send image data to server: {}", e);
                return;
            }

            // Signal to the server that all data has been sent and it can start processing
            if let Err(e) = server_stream.shutdown().await {
                eprintln!("Client Middleware: Failed to shutdown write stream: {}", e);
                return;
            }

            // Receive encrypted response from the server
            let mut response = Vec::new();
            if let Err(e) = server_stream.read_to_end(&mut response).await {
                eprintln!("Client Middleware: Failed to read encrypted response from server: {}", e);
                return;
            }

            // Send the encrypted response back to the client
            if let Err(e) = tx.send(response).await {
                eprintln!("Client Middleware: Failed to send response to client: {}", e);
            }
        }
    } else {
        eprintln!("Client Middleware: No servers are available to handle the request.");
    }
}

async fn find_available_server(server_ips: &[String]) -> Option<TcpStream> {
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

