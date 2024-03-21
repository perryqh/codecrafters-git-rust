use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use flate2::{write::ZlibEncoder, Compression};
use sha1::{Digest, Sha1};
use tempfile::tempdir;

use crate::config::Config;
use crate::git::Git;

pub(crate) fn build_test_git() -> anyhow::Result<TestGit> {
    let temp_dir = tempdir()?;
    let writer = Vec::new();
    let error_writer = Vec::new();
    let config = Config {
        writer,
        error_writer,
        dot_git_path: temp_dir.path().to_path_buf().join(".git"),
    };
    Ok(Git { config })
}

pub type TestGit = Git<Vec<u8>, Vec<u8>>;

pub(crate) fn write_to_git_objects(
    git: &TestGit,
    file_contents: &[u8],
) -> anyhow::Result<(String, PathBuf)> {
    let mut hasher = Sha1::new();
    hasher.update(file_contents);
    let result = hasher.finalize();
    let hash = format!("{:x}", result);

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(file_contents)?;
    let compressed_bytes = encoder.finish()?;

    fs::create_dir_all(
        git.config
            .dot_git_path
            .as_path()
            .join("objects/")
            .join(&hash[..2]),
    )?;
    let file_path = git
        .config
        .dot_git_path
        .as_path()
        .join("objects/")
        .join(&hash[..2])
        .join(&hash[2..]);
    fs::write(&file_path, compressed_bytes).context("error writing {file_path}")?;

    Ok((hash, file_path))
}
