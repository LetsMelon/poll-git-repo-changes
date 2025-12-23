use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use ractor::{Actor, ActorProcessingErr, ActorRef, concurrency::Duration};
use tracing::log;

use crate::git::GitService;

#[derive(Debug)]
pub enum IndexerActorMessage {
    Index,
    AutoIndex(Duration),
    StartAutoIndex(Duration),
    StopAutoIndex,
}

pub struct IndexerActor;

#[derive(Debug)]
pub struct IndexerActorState {
    last_indexed: Option<Instant>,
    last_commit_hash: Option<String>,
    timer_interval: Option<Duration>,
    git_service: GitService,
}

pub struct IndexerActorArguments {
    git_url: String,
    dir_name: Option<String>,
}

impl IndexerActorArguments {
    pub fn new(git_url: String, dir_name: Option<String>) -> Self {
        Self { git_url, dir_name }
    }
}

async fn dir_exists<P: AsRef<Path>>(path: P) -> bool {
    match tokio::fs::metadata(path.as_ref()).await {
        Ok(meta) => meta.is_dir(),
        Err(_) => false,
    }
}

fn get_dir_name_from_url(git_url: &str) -> &str {
    git_url
        .rsplit('/')
        .next()
        .and_then(|s| s.strip_suffix(".git"))
        .unwrap_or(git_url)
}

#[async_trait::async_trait]
impl Actor for IndexerActor {
    type State = IndexerActorState;
    type Msg = IndexerActorMessage;
    type Arguments = IndexerActorArguments;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        arguments: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let dir_name = arguments
            .dir_name
            .unwrap_or_else(|| get_dir_name_from_url(&arguments.git_url).to_string());

        let git_service = GitService::new(PathBuf::from(&dir_name));

        let last_commit_hash = if !dir_exists(&dir_name).await {
            log::info!(
                "Cloning repository from {} into {}",
                arguments.git_url,
                dir_name
            );

            git_service
                .clone_repository(&arguments.git_url)
                .await
                .map_err(|e| format!("Failed to clone repository: {:?}", e))?;

            None
        } else {
            log::info!("Repository already cloned in {}, skipping", &dir_name);

            git_service
                .get_current_commit_hash_from_fetch_head()
                .await
                .map_err(|e| format!("Failed to get commit hash: {:?}", e))?
        };

        Ok(IndexerActorState {
            last_indexed: None,
            last_commit_hash,
            timer_interval: None,
            git_service,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        log::info!("Handling message: '{:?}'", message);

        match message {
            IndexerActorMessage::Index => {
                state.last_indexed = Some(Instant::now());

                // pull latest changes from remote
                state.git_service.fetch().await.unwrap();

                // latest commit hash
                let current_commit_hash = state
                    .git_service
                    .get_current_commit_hash_from_fetch_head()
                    .await
                    .unwrap();
                match (&state.last_commit_hash, &current_commit_hash) {
                    (None, None) => {
                        log::info!("No commits found in repository.");
                    }
                    (None, Some(current_commit)) => {
                        log::info!("Initial commit hash: {}", current_commit);
                    }
                    (Some(old_commit), None) => {
                        log::error!(
                            "Previously had commit hash {}, but now no commits found!",
                            old_commit
                        );
                    }
                    // diff with last_commit_hash
                    (Some(old_commit), Some(current_commit)) if old_commit != current_commit => {
                        log::debug!("Diffing commits {} -> {}", old_commit, current_commit);

                        let patches = state
                            .git_service
                            .diff_commits(&old_commit, &current_commit)
                            .await
                            .unwrap();

                        for patch in patches {
                            // TODO store in database or do something with it
                            log::debug!("Patch: {:?}", patch);
                        }
                    }
                    (Some(_), Some(_)) => {
                        log::info!("No new commits to index.");
                    }
                }

                state.last_commit_hash = current_commit_hash;
            }
            IndexerActorMessage::AutoIndex(duration) => {
                // check if the auto index originated from the current interval
                if let Some(interval) = state.timer_interval
                    && duration == interval
                {
                    myself.cast(IndexerActorMessage::Index)?;

                    // schedule next auto-index
                    myself.send_after(interval, move || IndexerActorMessage::AutoIndex(duration));
                } else {
                    log::info!("Auto-indexing interval changed or stopped, not indexing.");
                }
            }
            IndexerActorMessage::StartAutoIndex(duration) => {
                log::info!("Starting auto-indexing every {:?}.", duration);
                state.timer_interval = Some(duration);
                myself.send_after(duration, move || IndexerActorMessage::AutoIndex(duration));
            }
            IndexerActorMessage::StopAutoIndex => {
                log::info!("Stopping auto-indexing.");
                state.timer_interval = None;
            }
        }

        Ok(())
    }
}
