use std::{
    path::PathBuf,
    process::{ExitStatus, Stdio},
};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};
use tracing::{instrument, log};

#[derive(Debug)]
pub enum GitError {
    CommandError(std::io::Error),
}

impl From<std::io::Error> for GitError {
    fn from(err: std::io::Error) -> Self {
        GitError::CommandError(err)
    }
}

#[derive(Debug)]
pub struct GitService {
    repository_path: PathBuf,
}

impl GitService {
    pub fn new(repository_path: PathBuf) -> Self {
        Self { repository_path }
    }

    #[instrument(skip(self))]
    async fn call_command(
        &self,
        program: &str,
        args: &[&str],
        run_in_parent: bool,
    ) -> Result<ExitStatus, std::io::Error> {
        let program = program.to_string();

        let mut child = Command::new(program.clone().as_str())
            .args(args)
            .current_dir(if run_in_parent {
                self.repository_path.parent().unwrap()
            } else {
                &self.repository_path
            })
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

    pub async fn clone_repository(&self, git_url: &str) -> Result<(), GitError> {
        let status = self
            .call_command(
                "git",
                &[
                    "clone",
                    "--filter=blob:none",
                    "--bare",
                    &git_url,
                    self.repository_path.file_name().unwrap().to_str().unwrap(),
                ],
                true,
            )
            .await?;

        if status.success() {
            Ok(())
        } else {
            Err(GitError::CommandError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Git clone command failed with exit status: {}", status),
            )))
        }
    }

    pub async fn fetch(&self) -> Result<(), GitError> {
        let status = self.call_command("git", &["fetch", "--all"], false).await?;

        if status.success() {
            Ok(())
        } else {
            Err(GitError::CommandError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Git fetch command failed with exit status: {}", status),
            )))
        }
    }

    pub async fn get_current_commit_hash_from_rev(
        &self,
        rev: &str,
    ) -> Result<Option<String>, GitError> {
        let out = Command::new("git")
            .arg("rev-parse")
            .arg(rev)
            .current_dir(&self.repository_path)
            .output()
            .await?;

        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            Ok(Some(stdout.trim().to_string()))
        } else {
            Ok(None)
        }
    }

    pub async fn get_current_commit_hash_from_fetch_head(
        &self,
    ) -> Result<Option<String>, GitError> {
        Ok(self.get_current_commit_hash_from_rev("FETCH_HEAD").await?)
    }

    pub async fn diff_commits_name_only(
        &self,
        c1: &str,
        c2: &str,
    ) -> Result<Vec<String>, GitError> {
        let out = Command::new("git")
            .args(&["diff", "--name-only", c1, c2])
            .current_dir(&self.repository_path)
            .output()
            .await?;

        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let files: Vec<String> = stdout.lines().map(|line| line.to_string()).collect();
            Ok(files)
        } else {
            Err(GitError::CommandError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Git diff command failed with exit status: {}", out.status),
            )))
        }
    }
}
