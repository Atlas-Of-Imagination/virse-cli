//! The application, not the SDK, owns environment and output policy.
use futures_util::StreamExt;
use virse_api::Client;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = std::env::var("VIRSE_BACKEND_URL")?;
    let token = std::env::var("VIRSE_API_TOKEN")?;
    let client = Client::new(&backend, &token)?;
    println!("{}", client.me().await?);
    if let (Ok(canvas), Ok(task)) = (
        std::env::var("VIRSE_EXAMPLE_CANVAS"),
        std::env::var("VIRSE_EXAMPLE_TASK"),
    ) {
        let stream = client.watch(canvas.parse()?, task.parse()?);
        futures_util::pin_mut!(stream);
        while let Some(snapshot) = stream.next().await {
            println!("{}", serde_json::to_string(&snapshot?)?);
        }
    }
    Ok(())
}
