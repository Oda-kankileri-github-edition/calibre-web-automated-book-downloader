mod config;

use config::CONFIG;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Access configuration settings using the global CONFIG instance
    println!("Base Directory: {:?}", CONFIG.base_dir);
    Ok(())
}
