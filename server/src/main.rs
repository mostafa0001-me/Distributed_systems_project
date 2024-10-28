use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use image::{DynamicImage, RgbaImage, GenericImageView, FilterType, ImageOutputFormat};
use std::io::Cursor;
use steganography::encoder::Encoder;

fn main() {
    let listener = TcpListener::bind("127.0.0.1:8080").expect("Could not bind server to port");

    for stream in listener.incoming() {
        let stream = stream.expect("Failed to establish connection");
        handle_client(stream);
    }
}

// Function to handle incoming requests and perform steganography encryption
fn handle_client(mut stream: TcpStream) {
    let mut buffer = Vec::new();
    
    // Read the real image data sent by the client middleware
    stream.read_to_end(&mut buffer).expect("Failed to read image");

    // Save the received real image temporarily
    let temp_image_path = "/home/mostafa/Distributed-Systems-Project/server/tempor.png";
    std::fs::write(temp_image_path, &buffer).expect("Failed to save image");

    // Load the real image
    let real_img = image::open(temp_image_path).expect("Failed to open real image");

    // Load the default image
    let default_image_path = "/home/mostafa/background.png";
    let default_img = image::open(default_image_path).expect("Failed to open default image");
    println!("HERE");
    // Resize the default image to be larger than the real image
    let resized_default_img = resize_default_image_to_fit(&real_img, default_img);
    println!("HERE0");
    // Perform steganography: Embed the real image into the resized default image
    // Compress the real image into a buffer
    let real_image_buffer = compress_image_to_buffer(&real_img);
    println!("HERE1");
    // Perform steganography: Embed the buffer of the real image into the resized default image
    let encrypted_image = embed_image_buffer_in_default(resized_default_img, &real_image_buffer);
    println!("HERE2");
    // Save the encrypted (embedded) image to a temporary file
    let encrypted_image_path = "/home/mostafa/Distributed-Systems-Project/server/enc.png";
    encrypted_image.save(encrypted_image_path).expect("Failed to save encrypted image");
    
    // Read the encrypted image and send it back to the client middleware
    let encrypted_image_data = std::fs::read(encrypted_image_path).expect("Failed to read encrypted image");
    stream.write_all(&encrypted_image_data).expect("Failed to send encrypted image");

    // Cleanup temporary files
    std::fs::remove_file(temp_image_path).expect("Failed to remove temp image");
}

// Function to resize the default image to fit the real image and leave space for embedding
fn resize_default_image_to_fit(real_img: &DynamicImage, default_img: DynamicImage) -> DynamicImage {
    let (real_width, real_height) = real_img.dimensions();

    // Resize the default image to be larger than the real image by a margin
    let new_width = (real_width as f32 * 2.0) as u32;
    let new_height = (real_height as f32 * 2.0) as u32;

    // Resize the default image to be larger than the real image
    default_img.resize(new_width, new_height, FilterType::Lanczos3)
}

// Function to embed the real image inside the default image using steganography
// Function to compress the real image to a buffer
fn compress_image_to_buffer(real_img: &DynamicImage) -> Vec<u8> {
    let mut buffer = Vec::new();
    real_img.write_to(&mut Cursor::new(&mut buffer), ImageOutputFormat::PNG)
        .expect("Failed to write image to buffer");
    buffer
}

// Function to embed the buffer of the real image inside the default image using steganography
fn embed_image_buffer_in_default(default_img: DynamicImage, real_image_buffer: &[u8]) -> DynamicImage {
    let default_rgba_img: RgbaImage = default_img.to_rgba();  // Convert default image to RGBA format

    if real_image_buffer.len() > default_rgba_img.len() {
        panic!("Default image is not large enough to embed the real image buffer.");
    }

    let encoder = Encoder::new(real_image_buffer, DynamicImage::ImageRgba8(default_rgba_img.clone()));
    let encoded_img = encoder.encode_alpha();
    DynamicImage::ImageRgba8(encoded_img)
}

