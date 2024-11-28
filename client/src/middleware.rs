// src/middleware.rs

use tokio::net::TcpStream;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::sync::mpsc::{Receiver, Sender};
use uuid::Uuid;
use crate::client::{ImageRequest, Request, Response, SignInRequest, SignUpRequest, SignUpResponse};
use crate::client::ImageResponse;
use bincode;
use serde::{Serialize, Deserialize};
use serde_json;

#[derive(Clone, Serialize, Deserialize)]
pub struct LightRequest {
    pub client_ip: String, // Change that to client_id?
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
      //  println!("Inside while loop of run middleware");
        let server_ips_clone = server_ips.clone();
        match request {
            Request::SignUp(req) => {
                sign_up(tx.clone(), req, server_ips_clone).await;
            },
            Request::SignIn(req) => {
                sign_in(tx.clone(), req, server_ips_clone).await;
            },
            Request::SignOut(req) => {
            },
            Request::ImageRequest(req) => {
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
    let client_ip = request.client_ip.clone();
    let request_id = request.request_id.clone();
    let request = Request::ImageRequest(request);
    println!("Encrypt image function handler at the middleware!");
    send_request_receive_response_client_server(tx, server_ips, client_ip, request, request_id).await; 
}

async fn sign_up(
    tx: Sender<Response>,
    request: SignUpRequest,
    server_ips: Vec<String>
) {
    let client_ip = request.client_ip.clone();
    let request_id = Uuid::new_v4().to_string();
    let request = Request::SignUp(request);
    println!("Sign up function handler at the middleware!");
    send_request_receive_response_client_server(tx, server_ips, client_ip, request, request_id).await; 
}

async fn sign_in(
    tx: Sender<Response>,
    request: SignInRequest,
    server_ips: Vec<String>
) {
    let client_id = request.client_id.clone();
    let request_id = Uuid::new_v4().to_string();
    let request = Request::SignIn(request);
  //  println!("Sign in request function handler at the middleware!");
    send_request_receive_response_client_server(tx, server_ips, client_id, request, request_id).await; 
}

// Send a generic request from client middleware to server middleware as well as receive response
async fn send_request_receive_response_client_server(
    tx: Sender<Response>,
    server_ips: Vec<String>,
    client_ip_id: String,
    request: Request,
    request_id: String
) {    
    tokio::spawn(async move {
        // Attempt to connect to an available server
        if let Some(mut stream) = find_available_server_t(&server_ips, &client_ip_id, &request_id).await {
            match request {
                Request::SignIn(_) => {
                    // Do not print anything for SignIn requests
                },
                _ => {
                    // Print IP of server
                    println!(
                        "Client Middleware: Client Connected {} to server {} for request number {}.",
                        client_ip_id, stream.peer_addr().unwrap(), request_id
                    );
                }
            }

            // Serialize the request to send over TCP
            let serialized_request = serde_json::to_string(&request).unwrap();
   
            // Send the serialized data to the server
            if let Err(e) = stream.write_all(serialized_request.as_bytes()).await {
                eprintln!("Client Middleware: Failed to send request to server: {}", e);
                return;
            }

            // Signal the server to start processing
            if let Err(e) = stream.shutdown().await {
                eprintln!("Client Middleware: Failed to shutdown write stream: {}", e);
                return;
            }

            // Receive the response from the server
            let mut buffer = Vec::new();

            match stream.read_to_end(&mut buffer).await {
                Ok(_) if !buffer.is_empty() => {
                    match serde_json::from_slice::<Response>(&buffer) {
                        Ok(response) => {
                            match response {
                                Response::SignUp(res) => {
                                    if let Err(e) = tx.clone().send(Response::SignUp(res)).await {
                                        eprintln!("Client Middleware: Failed to send response to client: {}", e);
                                    }
                                },
                                Response::SignIn(res) => {
                                    if let Err(e) = tx.clone().send(Response::SignIn(res)).await {
                                        eprintln!("Client Middleware: Failed to send response to client: {}", e);
                                    }
                                },
                                Response::ImageResponse(res) => {
                                    if let Err(e) = tx.clone().send(Response::ImageResponse(res)).await {
                                        eprintln!("Client Middleware: Failed to send response to client: {}", e);
                                    }
                                },
                                Response::SignOut(res) => {
                                    if let Err(e) = tx.clone().send(Response::SignOut(res)).await {
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
    client_ip:  &String,
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
            //        println!("Client Middleware: Server at {} accepted the request", ip);
                    return Some(stream);
                }
            }
        }
    }
    None
}

