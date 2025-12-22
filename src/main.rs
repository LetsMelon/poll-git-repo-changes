use async_trait::async_trait;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub mod actor;

pub enum MyFirstActorMessage {
    AddCrate(String),
}

pub struct MyFirstActor;

pub struct MyFirstActorState {
    crates: Vec<String>,
    actor_ref: ActorRef<CrateActorMessage>,
}

impl MyFirstActorState {
    fn new(actor_ref: ActorRef<CrateActorMessage>) -> Self {
        Self {
            crates: Vec::new(),
            actor_ref,
        }
    }
}

#[async_trait]
impl Actor for MyFirstActor {
    type State = MyFirstActorState;
    type Msg = MyFirstActorMessage;
    type Arguments = ActorRef<CrateActorMessage>;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        arguments: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(MyFirstActorState::new(arguments))
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            MyFirstActorMessage::AddCrate(crate_name) => {
                state.crates.push(crate_name.clone());
                state.actor_ref.cast(CrateActorMessage::Fetch(crate_name))?;
            }
        }

        Ok(())
    }
}

pub enum CrateActorMessage {
    Fetch(String),
}

pub struct CrateActor;

#[async_trait]
impl Actor for CrateActor {
    type State = ();
    type Msg = CrateActorMessage;
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _arguments: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(())
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            CrateActorMessage::Fetch(crate_name) => {
                println!("Fetching crate: {}", crate_name);
            }
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() {
    /*
    let (crate_actor, crate_handle) = Actor::spawn(None, CrateActor, ())
        .await
        .expect("CrateActor failed to start");

    let (actor, actor_handle) = Actor::spawn(None, MyFirstActor, crate_actor.clone())
        .await
        .expect("MyFirstActor failed to start");

    actor
        .cast(MyFirstActorMessage::AddCrate("serde".into()))
        .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    actor.stop(None);
    crate_actor.stop(None);

    actor_handle.await.unwrap();
    crate_handle.await.unwrap();
    */

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::builder().parse_lossy("debug"))
        .init();

    let (indexer_actor, indexer_handle) = Actor::spawn(
        None,
        actor::IndexerActor,
        "https://github.com/rust-lang/crates.io-index.git".to_string(),
    )
    .await
    .unwrap();

    indexer_actor
        .cast(actor::IndexerActorMessage::Index)
        .unwrap();
    tokio::time::sleep(tokio::time::Duration::from_mins(5)).await;

    indexer_actor
        .cast(actor::IndexerActorMessage::Index)
        .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;

    indexer_actor.stop(None);
    indexer_handle.await.unwrap();
}
