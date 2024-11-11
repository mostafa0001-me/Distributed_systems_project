use tokio::sync::mpsc::{Receiver, Sender};
use tokio::fs;
use uuid::Uuid;
use tokio::task;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::time::{Instant};
use tokio::sync::Mutex as TokioMutex;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use once_cell::sync::Lazy;

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

pub async fn run_client(
    tx: Sender<ImageRequest>,
    mut rx: Receiver<ImageResponse>,
    client_ip: String,
    n: u32,
) {
    let image_paths = vec!["pic0.png"];
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
    let mut j: i32 = 0;
    while let Some(response) = rx.recv().await {
        let encrypted_image_path =
            format!("encrypted_pic_{}_{}.png", j, &response.request_id);
        if let Err(e) = fs::write(&encrypted_image_path, &response.encrypted_image_data).await {
            eprintln!(
                "Failed to save encrypted image {} {}: {}",
                j, &response.request_id, e
            );
        } else {
            println!(
                "Client: Encrypted image {} {} saved!",
                j, &response.request_id
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
        j += 1;
    }
}