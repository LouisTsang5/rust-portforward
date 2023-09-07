#![allow(non_snake_case)]

use futures::io;
use tokio::{fs::File, io::AsyncReadExt};

pub mod Config;
pub mod ConnHandle;
pub mod Meter;

async fn fill_random_bytes(buff: &mut [u8]) -> Result<(), io::Error> {
    let mut file = File::open("/dev/urandom").await?; // Linux-specific, use appropriate platform-specific source
    file.read_exact(buff).await?;
    Ok(())
}
