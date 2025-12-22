use std::{sync::atomic::AtomicBool, time::Instant};

use gix::{
    ObjectDetached, Repository, ThreadSafeRepository, clone::PrepareFetch, create::Kind, progress,
    remote,
};
use ractor::{Actor, ActorProcessingErr, ActorRef};

#[derive(Debug)]
pub enum IndexerActorMessage {
    Index,
}

pub struct IndexerActor;

#[derive(Debug)]
pub struct IndexerActorState {
    git_url: String,
    repository: ThreadSafeRepository,
    last_indexed: Option<Instant>,
    last_commit: Option<ObjectDetached>,
}

#[async_trait::async_trait]
impl Actor for IndexerActor {
    type State = IndexerActorState;
    type Msg = IndexerActorMessage;
    type Arguments = String;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        arguments: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let repo = if let Some(repo) = ThreadSafeRepository::open("./crates.io-index.git").ok() {
            println!("Reusing existing repository");
            repo.to_owned()
        } else {
            println!("Cloning repository afresh");
            let out = PrepareFetch::new(
                arguments.clone(),
                "./crates.io-index",
                Kind::WithWorktree,
                Default::default(),
                Default::default(),
            )
            .unwrap()
            .fetch_then_checkout(progress::Discard, &AtomicBool::new(false))
            .unwrap();

            let sth = out.0.persist();

            sth.into_sync()
        };

        Ok(IndexerActorState {
            git_url: arguments,
            repository: repo,
            last_indexed: None,
            last_commit: None,
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        println!("Received message: {:?}", message);

        match message {
            IndexerActorMessage::Index => {
                state.last_indexed = Some(Instant::now());

                let repo: Repository = state.repository.to_thread_local();

                // fetch latest changes
                repo.find_remote("origin")?
                    .connect(remote::Direction::Fetch)
                    .unwrap()
                    .prepare_fetch(progress::Discard, Default::default())
                    .unwrap()
                    .receive(progress::Discard, &AtomicBool::new(false))
                    .unwrap();

                let old = state
                    .last_commit
                    .clone()
                    .map(|object| object.attach(&repo).into_commit());
                let new = repo.head_commit()?;

                // get diff since last commit
                if let Some(old_commit) = old {
                    let diff = repo
                        .diff_tree_to_tree(
                            Some(&old_commit.tree().unwrap()),
                            Some(&new.tree().unwrap()),
                            None,
                        )
                        .unwrap();

                    dbg!(diff);
                }

                // set last commit to current head
                state.last_commit = Some(new.detach());

                dbg!(&state);
            }
        }
        Ok(())
    }
}
