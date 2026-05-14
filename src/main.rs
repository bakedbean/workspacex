mod config;
mod error;
mod store;
mod names;
mod git;
mod setup;
mod repo;
mod workspace;
mod pty;

fn main() -> error::Result<()> {
    println!("wsx 0.1.0");
    Ok(())
}
