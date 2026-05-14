mod config;
mod error;
mod store;
mod names;
mod git;

fn main() -> error::Result<()> {
    println!("wsx 0.1.0");
    Ok(())
}
