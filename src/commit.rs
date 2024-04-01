use std::path::Path;

use anyhow::Context;

use crate::tree::{commit_tree, write_tree_for};

pub(crate) fn commit(
    dot_git_path: &Path,
    path: &Path,
    message: &str,
) -> anyhow::Result<Option<[u8; 20]>> {
    let head_ref =
        std::fs::read_to_string(dot_git_path.join("HEAD")).with_context(|| format!("read HEAD"))?;
    let Some(head_ref) = head_ref.strip_prefix("ref: ") else {
        anyhow::bail!("refusing to commit onto detached HEAD");
    };
    let head_ref = head_ref.trim();
    let parent_hash = std::fs::read_to_string(format!(".git/{head_ref}"))
        .with_context(|| format!("read HEAD reference target '{head_ref}'"))?;
    let parent_hash = parent_hash.trim();

    let Some(tree_hash) = write_tree_for(dot_git_path, path).context("write tree")? else {
        eprintln!("not committing empty tree");
        return Ok(None);
    };
    let commit_hash = commit_tree(
        dot_git_path,
        &message,
        &hex::encode(tree_hash),
        Some(parent_hash),
    )
    .context("create commit")?;

    match commit_hash {
        Some(commit_hash) => {
            std::fs::write(
                dot_git_path.join(format!("{head_ref}")),
                &hex::encode(commit_hash),
            )
            .with_context(|| format!("update HEAD reference target '{head_ref}'"))?;
            Ok(Some(commit_hash))
        }
        None => {
            eprintln!("failed to commit tree");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};
    use tempfile::tempdir;

    use crate::{config::Config, git::Git};

    use super::*;

    #[test]
    fn test_commit_tree_complex() -> anyhow::Result<()> {
        let tmp_dir = tempdir()?;
        let dot_git = tmp_dir.path().join("dot-git");
        let config = Config {
            writer: Vec::new(),
            error_writer: Vec::new(),
            dot_git_path: dot_git.clone(),
        };
        Git { config }.init()?;
        fs::create_dir_all(dot_git.join("refs/heads"))?;
        let staging_git_dir = PathBuf::from(format!("tests/fixtures/complex-app"));
        let result = write_tree_for(&dot_git, staging_git_dir.as_path());
        assert!(&result.is_ok());
        let tree_sha = hex::encode(result.unwrap().unwrap());
        assert_eq!(tree_sha, "f33421767929a06951899aa91cc699df29c3893b");
        let result = commit(&dot_git, &staging_git_dir, "initial commit")?;
        let commit_sha = hex::encode(result.unwrap());
        assert_eq!(commit_sha.len(), 40);

        dbg!(&dot_git);
        let head = fs::read_to_string(dot_git.join("HEAD"))?;
        assert_eq!(head, "ref: refs/heads/master\n");

        Ok(())
    }
}
