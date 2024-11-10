use tokio::sync::mpsc::{Receiver, Sender};
use std::fs;
use uuid::Uuid;
use tokio::task;
use serde::{Serialize, Deserialize};

// Define a struct for requests, including a unique ID and the image data
#[derive(Clone, Serialize, Deserialize)]
pub struct ImageRequest {
    pub client_ip : String,
    pub request_id: String,
    pub image_data: Vec<u8>,
}

pub async fn run_client(tx: Sender<ImageRequest>, mut rx: Receiver<Vec<u8>>, client_ip : String, n: u32) {
    let image_paths = vec![
        "pic0.png",
    ];

    let l: u32 = image_paths.len() as u32;
    let mut i: u32 = 0;

    while i < n {
        let image_path = image_paths[(i % l) as usize];
        // Read the image file
        let image_data = fs::read(image_path).expect("Failed to read image file");

        // Generate a unique request ID
        let request_id = Uuid::new_v4().to_string();

        let client_ip = client_ip.clone();
        // Create an ImageRequest with the unique request ID
        let request = ImageRequest {
            client_ip: client_ip.clone(),
            request_id: request_id.clone(),
            image_data,
        };

        // Spawn a task to send the image concurrently
        let tx_clone = tx.clone();
        task::spawn(async move {
            if let Err(e) = tx_clone.send(request).await {
                eprintln!("Client: Failed to send image to middleware: {}", e);
            } else {
                println!("Client: Image data with request ID {} sent to middleware.", request_id);
            }
        });

        i += 1;
    }

    // Receive responses from the middleware and save each response
    let mut j = 0;
    while let Some(response) = rx.recv().await {
        let encrypted_image_path = format!("encrypted_pic_{}.png", j);
        fs::write(&encrypted_image_path, response).expect("Failed to save encrypted image");
        println!("Client: Encrypted image {} saved!", j);
        j += 1;
    }
}
