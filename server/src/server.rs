use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task;
use std::io::Cursor;
use image::{DynamicImage, ImageOutputFormat};
use steganography::encoder::Encoder;

pub async fn run_server(mut rx: Receiver<Vec<u8>>, tx: Sender<Vec<u8>>) {
    // Load the pre-resized default image directly
    let default_image_path = "default_resized.png";
    let default_img = image::open(default_image_path).expect("Failed to open default image");

    while let Some(data) = rx.recv().await {
        let default_img = default_img.clone();  // Clone the default image for thread safety
        let tx = tx.clone();
        println!("Using pre-resized default image");

        // Spawn a new task for each encryption task
        task::spawn(async move {
            // Encrypt (embed) the image buffer into the default image
            let encrypted_data = embed_image_buffer_in_default(default_img, &data);
            println!("Encryption done");

            // Write encrypted image data to buffer
            let mut encrypted_image_buffer = Vec::new();
            encrypted_data
                .write_to(&mut Cursor::new(&mut encrypted_image_buffer), ImageOutputFormat::PNG)
                .expect("Failed to write encrypted image to buffer");

            // Send encrypted data back to the server middleware
            tx.send(encrypted_image_buffer).await.expect("Failed to send encrypted data to server middleware");
        });
    }
}

// // Function to resize the default image to fit the real image and leave space for embedding
// fn resize_default_image_to_fit(default_img: DynamicImage) -> DynamicImage {
//     let (real_width, real_height) = default_img.dimensions();

//     // Resize the default image to be larger than the real image by a margin
//     let new_width = (real_width as f32 * 2.0) as u32;
//     let new_height = (real_height as f32 * 2.0) as u32;

//     // Resize the default image to be larger than the real image
//     default_img.resize(new_width, new_height, FilterType::Lanczos3)
// }

// Function to embed real image buffer into the default image's alpha channel
fn embed_image_buffer_in_default(default_img: DynamicImage, real_image_buffer: &[u8]) -> DynamicImage {
    //let default_rgba_img: RgbaImage = default_img.to_rgba(); // Convert default image to RGBA format


    let encoder = Encoder::new(real_image_buffer, default_img);
    let encoded_img = encoder.encode_alpha();
    DynamicImage::ImageRgba8(encoded_img)
}
