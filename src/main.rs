use std::time::Duration;

use ractor::Actor;
use tokio::time::Duration as TDuration;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::actor::{IndexerActor, IndexerActorArguments, IndexerActorMessage};

pub mod actor;
pub mod git;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::builder().parse_lossy("debug"))
        .init();

    let (indexer_actor, indexer_handle) = Actor::spawn(
        None,
        IndexerActor,
        IndexerActorArguments::new(
            "https://github.com/rust-lang/crates.io-index.git".to_string(),
            None,
        ),
    )
    .await
    .unwrap();

    indexer_actor
        .cast(IndexerActorMessage::StartAutoIndex(Duration::from_secs(25)))
        .unwrap();

    tokio::time::sleep(TDuration::from_mins(1)).await;

    indexer_actor
        .cast(IndexerActorMessage::StartAutoIndex(Duration::from_secs(10)))
        .unwrap();

    tokio::time::sleep(TDuration::from_mins(1)).await;

    indexer_actor
        .cast(IndexerActorMessage::StopAutoIndex)
        .unwrap();

    tokio::time::sleep(TDuration::from_millis(50)).await;

    indexer_actor.stop(None);
    indexer_handle.await.unwrap();
}
