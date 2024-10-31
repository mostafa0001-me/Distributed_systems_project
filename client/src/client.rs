use tokio::sync::mpsc::{Receiver, Sender};
use std::fs;


pub async fn run_client(tx: Sender<Vec<u8>>, mut rx: Receiver<Vec<u8>>) {
    let image_path = "/home/mostafa/Distributed/client/pic0.png";

    // Read the image file
    let image_data = fs::read(image_path).expect("Failed to read image file");

    // Send image data to middleware
    if let Err(e) = tx.send(image_data).await {
        eprintln!("Client: Failed to send image to middleware: {}", e);
        return;
    }
    println!("Client: Image data sent to middleware.");

    // Receive responses from the middleware and save each response
    let mut i = 0;
    while let Some(response) = rx.recv().await {
        let encrypted_image_path = format!("../encrypted_pic_{}.png", i);
        fs::write(&encrypted_image_path, response).expect("Failed to save encrypted image");
        println!("Client: Encrypted image {} saved!", i);
        i += 1;
    }
}

