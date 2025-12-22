use std::time::Duration;

use ractor::Actor;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::actor::IndexerActorArguments;

pub mod actor;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::builder().parse_lossy("debug"))
        .init();

    let (indexer_actor, indexer_handle) = Actor::spawn(
        None,
        actor::IndexerActor,
        IndexerActorArguments::new(
            "https://github.com/rust-lang/crates.io-index.git".to_string(),
            None,
            Duration::from_secs(45),
        ),
    )
    .await
    .unwrap();

    // TODO also implement StartAutoIndex and StopAutoIndex messages
    indexer_actor
        .cast(actor::IndexerActorMessage::AutoIndex)
        .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_mins(10)).await;

    indexer_actor.stop(None);
    indexer_handle.await.unwrap();
}
