#[tokio::main]
async fn main() -> nactl::error::AppResult<()> {
    nactl::interface::cli::run().await
}
