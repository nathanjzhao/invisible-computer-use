use computer_history::{store::Store, Error, Result};
use std::{
    io::{self, BufReader},
    path::PathBuf,
};
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.iter().any(|v| v == "--help") || args.is_empty() {
        println!("Usage: computer-history mcp --root PATH\nThe control server does not start capture. Enable capture in the desktop app.");
        return Ok(());
    }
    if args.len() != 3 || args[0] != "mcp" || args[1] != "--root" {
        return Err(Error::invalid("Use mcp --root /absolute/history/path"));
    }
    let store = Store::open(PathBuf::from(&args[2]))?;
    computer_history::mcp::serve(&mut BufReader::new(io::stdin()), &mut io::stdout(), store)
        .map_err(|e| Error::new("computerHistoryProtocolError", e.to_string()))
}
