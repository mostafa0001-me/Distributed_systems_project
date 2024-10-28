use steganography::decoder::Decoder;
use steganography::encoder::Encoder;
use image::{DynamicImage, RgbaImage, GenericImageView, FilterType, ImageOutputFormat};
use std::io::Cursor;
use std::fs;

fn main() {
    // File paths for the real image and default background image
    let real_image_path = "/home/mostafa/Distributed-Systems-Project/client/4k_image.png";
    let default_image_path = "/home/mostafa/background.png";
    let encrypted_image_path = "/home/mostafa/Distributed-Systems-Project/server/enc_dep.png";
    let decrypted_image_path = "/home/mostafa/Distributed-Systems-Project/server/real_image_decrypted.png";

    // Step 1: Encrypt the real image into the default image
    let real_img = image::open(real_image_path).expect("Failed to open real image");
    let default_img = image::open(default_image_path).expect("Failed to open default image");

    // Resize default image to fit real image
    let resized_default_img = resize_default_image_to_fit(&real_img, default_img);

    // Compress the real image into a buffer
    let real_image_buffer = compress_image_to_buffer(&real_img);

    // Perform steganography: Embed the buffer of the real image into the resized default image
    let encrypted_image = embed_image_buffer_in_default(resized_default_img, &real_image_buffer);

    // Save the encrypted image
    encrypted_image.save(encrypted_image_path).expect("Failed to save encrypted image");

    // Step 2: Decrypt the hidden image buffer from the encrypted image
    let encrypted_img = image::open(encrypted_image_path).expect("Failed to open encrypted image");

    // Extract the hidden image data from the encrypted image
    let real_image_data = extract_hidden_image_buffer_from_encoded(encrypted_img);

    // Save the decrypted image buffer as a PNG file for verification
    fs::write(decrypted_image_path, real_image_data).expect("Failed to save decrypted image data");

    println!("Encryption and decryption completed successfully.");
}

// Function to resize the default image to fit the real image
fn resize_default_image_to_fit(real_img: &DynamicImage, default_img: DynamicImage) -> DynamicImage {
    let (real_width, real_height) = real_img.dimensions();
    let new_width = (real_width as f32 * 2.0) as u32;
    let new_height = (real_height as f32 * 2.0) as u32;
    default_img.resize(new_width, new_height, FilterType::Lanczos3)
}

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

// Function to extract the hidden image buffer from the encoded image
fn extract_hidden_image_buffer_from_encoded(encoded_img: DynamicImage) -> Vec<u8> {
    let encoded_rgba_img: RgbaImage = encoded_img.to_rgba();  // Convert encoded image to RGBA format
    let decoder = Decoder::new(encoded_rgba_img);
    decoder.decode_alpha()
}

