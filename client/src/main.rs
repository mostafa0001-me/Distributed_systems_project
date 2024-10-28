use std::fs;
use std::io::prelude::*;
use std::net::{TcpStream, Shutdown};

fn main() {
    let image_path = "/home/mostafa/Distributed-Systems-Project/client/4k_image.png"; // Example image path
    let middleware_address = "127.0.0.1:7878"; // Middleware address

    // Read the 4K image from the file system
    let image_data = fs::read(image_path).expect("Failed to read image file");

    // Send the image data to the middleware
    let mut stream = TcpStream::connect(middleware_address).expect("Could not connect to middleware");

    // Write image data to the middleware
    stream.write_all(&image_data).expect("Failed to send image to middleware");
    stream.shutdown(Shutdown::Write).expect("Failed to shut down writing side of stream");

    // Receive the encrypted image from the middleware
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).expect("Failed to receive data from middleware");

    // Save or log the encrypted image
    fs::write("/home/mostafa/Distributed-Systems-Project/client/encrypted_image.png", buffer).expect("Failed to save encrypted image");
    println!("Image encrypted and saved!");
}

