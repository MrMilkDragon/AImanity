#[cfg(target_os = "linux")]
#[path = "../relay.rs"]
mod relay;

fn main() -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        relay::guest()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(std::io::Error::other("The AVF guest relay requires Linux"))
    }
}
