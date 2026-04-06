#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pirate_cli_core::run_from_env().await
}
