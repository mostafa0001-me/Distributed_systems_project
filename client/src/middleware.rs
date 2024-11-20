// src/middleware.rs

use tokio::net::TcpStream;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::sync::mpsc::{Receiver, Sender};
use crate::client::{ImageRequest, Request, Response, SignUpRequest, SignUpResponse};
use crate::client::ImageResponse;
use bincode;
use serde::{Serialize, Deserialize};
use serde_json;

#[derive(Clone, Serialize, Deserialize)]
pub struct LightRequest {
    pub client_ip: String,
    pub request_id: String,
    pub message_data: String,
}

/// Main function to handle image data transmission through an elected server
pub async fn run_middleware(
    mut rx: Receiver<Request>,
    tx: Sender<Response>,
    server_ips: Vec<String>
) {
    println!("Inside run middleware");
    while let Some(request) = rx.recv().await {
        // Clone tx and server_ips for use in the spawned task
        let server_ips_clone = server_ips.clone();
        println!("I am receiving something at the client middleware!");
        match request {
            Request::SignUp(req) => {
                println!("Sign up request received at the middleware!");
                sign_up(tx.clone(), req, server_ips_clone).await;
            },
            Request::SignIn(req) => {

            },
            Request::SignOut(req) => {

            },
            Request::ImageRequest(req) => {
                println!("Encrypt image received at the middleware!");
                encrypt_image_from_server(tx.clone(), req, server_ips_clone).await;
            },
            Request::ListContents => {
                
            },
        }
    }
}

async fn encrypt_image_from_server(
    tx: Sender<Response>,
    request: ImageRequest,
    server_ips: Vec<String>
) {
        // Spawn a new task to handle each request concurrently
        let client_ip = request.client_ip.clone();
        let tx_clone = tx.clone();
        let request_id = request.request_id.clone();
        let server_ips_clone = server_ips.clone();
        tokio::spawn(async move {
            // Attempt to connect to an available server
            if let Some(mut server_stream) = find_available_server_t(&server_ips_clone, &client_ip, &request_id).await {
                // Print IP of server
                println!(
                    "Client Middleware: Client Connected {} to server {} for request number {}.",
                    client_ip, server_stream.peer_addr().unwrap(), request_id
                );
                // Serialize the request to send over TCP
                let request = Request::ImageRequest(request);
                let serialized_request = serde_json::to_string(&request).unwrap();

                // Send the serialized data to the server
                if let Err(e) = server_stream.write_all(&serialized_request.as_bytes()).await {
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

                if let Ok(image_response) = bincode::deserialize::<ImageResponse>(&response) {
                    println!("Client Middleware: received response id {}", image_response.request_id);
                    // Send the response back to the client
                    if let Err(e) = tx_clone.send(Response::ImageResponse(image_response)).await {
                        eprintln!("Client Middleware: Failed to send response to client: {}", e);
                    }
                }
            } else {
                eprintln!("Client Middleware: No servers are available to handle the request.");
            }
        });
}


async fn sign_up(
    tx: Sender<Response>,
    request: SignUpRequest,
    server_ips: Vec<String>
) {
    let client_ip = request.client_ip.clone();
    let request_id = String::new();
    let tx_clone = tx.clone();
    let server_ips_clone = server_ips.clone();
    println!("Sign up request function handler at the middleware!");
    tokio::spawn(async move {
        // Attempt to connect to an available server
        if let Some(mut stream) = find_available_server_t(&server_ips_clone, &client_ip, &request_id).await {
            // Print IP of server
            println!(
                "Client Middleware: Client Connected {} to server {} for request number {}.",
                client_ip, stream.peer_addr().unwrap(), request_id
            );

            // Serialize the request to send over TCP
            let request = Request::SignUp(request);
            let serialized_request = serde_json::to_string(&request).unwrap();
            println!("Serialized Request {}", serialized_request);
   
            // Send the serialized data to the server
            if let Err(e) = stream.write_all(serialized_request.as_bytes()).await {
                eprintln!("Client Middleware: Failed to send image data to server: {}", e);
                return;
            }

            // Signal the server to start processing
            if let Err(e) = stream.shutdown().await {
                eprintln!("Client Middleware: Failed to shutdown write stream: {}", e);
                return;
            }

            // Receive the encrypted response from the server
            let mut buffer = Vec::new();

            match stream.read_to_end(&mut buffer).await {
                Ok(_) if !buffer.is_empty() => {
                    match serde_json::from_slice::<Response>(&buffer) {
                        Ok(response) => {
                            match response {
                                Response::SignUp(res) => {
                                    println!("Got response {}", res.client_id);
                                    if let Err(e) = tx_clone.send(Response::SignUp(res)).await {
                                        eprintln!("Client Middleware: Failed to send response to client: {}", e);
                                    }
                                },
                                _ => println!("Unexpected response."),
                            }
                        }
                        Err(err) => {
                            eprintln!("Failed to deserialize: {}", err);
                        }
                    }
                }
                Ok(_) => {
                    eprintln!("Empty buffer received!");
                }
                Err(err) => {
                    eprintln!("Failed to read from socket: {}", err);
                }
            }
        } else {
            eprintln!("Client Middleware: No servers are available to handle the request.");
        }
    });

   
}


/// Function to attempt to connect to any available server
async fn find_available_server_t(
    server_ips: &[String],
    client_ip: &String,
    request_id: &String
) -> Option<TcpStream> {
    for ip in server_ips {
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
                    continue;
                }
            };

            let mut buffer_send = Vec::new();
            buffer_send.extend_from_slice(&serialized_data);

            // Send the serialized data to the server
            if let Err(e) = stream.write(&buffer_send).await {
                eprintln!("Client Middleware: Failed to send data to server: {}", e);
                continue;
            }

            // Read the response from the server
            let mut ack_buffer = [0; 128];
            if let Ok(bytes_read) = stream.read(&mut ack_buffer).await {
                let response = String::from_utf8_lossy(&ack_buffer[..bytes_read]).to_string();
                if !response.is_empty() {
                    println!("Client Middleware: Server at {} accepted the request", ip);
                    return Some(stream);
                }
            }
        }
    }
    None
}

