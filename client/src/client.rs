use std::sync::mpsc::{Sender, Receiver};
use std::fs;

pub fn run_client(tx: Sender<Vec<u8>>, rx: Receiver<Vec<u8>>) {
    let image_path = "/home/mostafa/Distributed-Systems-Project/client/4k_image.png";

    // Read the image file
    let image_data = fs::read(image_path).expect("Failed to read image file");

    // Send image data to middleware
    tx.send(image_data).expect("Failed to send image to middleware");
    println!("Client: Image data sent to middleware.");

    // Receive responses from the middleware and save each response
    for (i, response) in rx.iter().enumerate() {
        let encrypted_image_path = format!("/home/mostafa/Distributed-Systems-Project/client/encrypted_image_{}.png", i);
        fs::write(&encrypted_image_path, response).expect("Failed to save encrypted image");
        println!("Client: Encrypted image {} saved!", i);
    }
}

