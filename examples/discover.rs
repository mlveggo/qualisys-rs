//! Broadcasts for QTM instances on the local network.

use std::time::Duration;

fn main() -> Result<(), qualisys::Error> {
    env_logger::init();

    let timeout = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(1));

    let servers = qualisys::discover::discover(timeout)?;
    if servers.is_empty() {
        println!("No QTM instances responded");
        return Ok(());
    }
    for server in servers {
        println!("{server}");
    }
    Ok(())
}
