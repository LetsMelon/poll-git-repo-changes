use std::{
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    sync::Arc,
    time::Instant,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};
use tracing::log;

use ractor::{Actor, ActorProcessingErr, ActorRef, concurrency::Duration};

#[derive(Debug)]
pub enum IndexerActorMessage {
    Index,
    AutoIndex,
}

pub struct IndexerActor;

#[derive(Debug)]
pub struct IndexerActorState {
    git_url: String,
    repository: PathBuf,
    last_indexed: Option<Instant>,
    last_commit_hash: Option<String>,
    timer_interval: Duration,
}

pub struct IndexerActorArguments {
    git_url: String,
    dir_name: Option<String>,
    timer_interval: Duration,
}

impl IndexerActorArguments {
    pub fn new(git_url: String, dir_name: Option<String>, timer_interval: Duration) -> Self {
        Self {
            git_url,
            dir_name,
            timer_interval,
        }
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

async fn call_command(
    program: &str,
    args: &[&str],
    working_dir: &Path,
) -> Result<ExitStatus, std::io::Error> {
    let program = Arc::new(program.to_string());

    let mut child = Command::new(program.clone().as_str())
        .args(args)
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // stdout -> debug
    let p = program.clone();
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log::debug!("{}: {}", p.as_str(), line);
        }
    });

    // stderr -> error
    let p = program.clone();
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log::error!("{}: {}", p.as_str(), line);
        }
    });

    let status = child.wait().await?;

    stdout_task.await?;
    stderr_task.await?;

    Ok(status)
}

async fn get_commit_hash_from_head(repo_path: &Path) -> Result<Option<String>, std::io::Error> {
    let out = Command::new("git")
        .arg("rev-parse")
        .arg("FETCH_HEAD")
        .current_dir(repo_path)
        .output()
        .await?;

    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        Ok(Some(stdout.trim().to_string()))
    } else {
        Ok(None)
    }
}

async fn fetch_repository(repo_path: &Path) -> Result<(), std::io::Error> {
    let status = call_command("git", &["fetch", "--all"], repo_path).await?;

    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Git fetch failed",
        ))
    }
}

async fn diff_commits(
    repo_path: &Path,
    old_commit: &str,
    new_commit: &str,
) -> Result<Vec<String>, std::io::Error> {
    let out = Command::new("git")
        .args(&["diff", "--name-only", old_commit, new_commit])
        .current_dir(repo_path)
        .output()
        .await?;

    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let files: Vec<String> = stdout.lines().map(|line| line.to_string()).collect();
        Ok(files)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Git diff failed",
        ))
    }
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

        let last_commit_hash = if !dir_exists(&dir_name).await {
            log::info!(
                "Cloning repository from {} into {}",
                arguments.git_url,
                dir_name
            );

            let status = call_command(
                "git",
                &[
                    "clone",
                    "--filter=blob:none",
                    "--bare",
                    &arguments.git_url,
                    &dir_name,
                ],
                std::env::current_dir().unwrap().as_path(),
            )
            .await
            .map_err(|e| format!("Failed to execute git command: {}", e))?;

            if !status.success() {
                return Err("Failed to clone repository".into());
            }

            None
        } else {
            log::info!("Repository already cloned in {}, skipping", &dir_name);

            get_commit_hash_from_head(&PathBuf::from(&dir_name))
                .await
                .map_err(|e| format!("Failed to get commit hash: {}", e))?
        };

        Ok(IndexerActorState {
            git_url: arguments.git_url,
            repository: PathBuf::from(dir_name),
            last_indexed: None,
            last_commit_hash,
            timer_interval: arguments.timer_interval,
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
            IndexerActorMessage::AutoIndex | IndexerActorMessage::Index => {
                state.last_indexed = Some(Instant::now());

                // pull latest changes from remote
                fetch_repository(&state.repository).await.unwrap();

                // latest commit hash
                let current_commit_hash =
                    get_commit_hash_from_head(&state.repository).await.unwrap();

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

                        let changed_files =
                            diff_commits(&state.repository, old_commit, current_commit)
                                .await
                                .unwrap();

                        for file in changed_files {
                            // TODO store in database
                            log::info!("Changed file: {}", file);
                        }
                    }
                    (Some(_), Some(_)) => {
                        log::info!("No new commits to index.");
                    }
                }

                state.last_commit_hash = current_commit_hash.clone();
            }
        }

        if matches!(message, IndexerActorMessage::AutoIndex) {
            myself.send_after(state.timer_interval, || IndexerActorMessage::AutoIndex);
        }

        Ok(())
    }
}
