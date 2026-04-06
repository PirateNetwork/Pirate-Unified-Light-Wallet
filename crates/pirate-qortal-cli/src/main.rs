#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pirate_cli_core::run_qortal_from_iter(std::env::args()).await
}
