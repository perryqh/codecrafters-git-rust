use std::{
    ffi::CStr,
    fmt::{self, Display},
    io::{BufRead, Read},
    path::PathBuf,
};

use anyhow::{bail, ensure, Context};

use crate::object::{Object, ObjectType};

#[derive(Debug, PartialEq, Eq, Default)]
pub enum TreeEntryType {
    Blob,
    #[default]
    Tree,
}

#[derive(Debug, PartialEq, Eq, Default)]
pub struct Tree {
    pub entries: Vec<TreeEntry>,
}

#[derive(Debug, PartialEq, Eq, Default)]
pub struct TreeEntry {
    pub mode: TreeEntryMode,
    pub name: String,
    pub sha: String,
}

impl TreeEntry {
    pub fn tree_entry_type(&self) -> TreeEntryType {
        match self.mode {
            TreeEntryMode::Directory => TreeEntryType::Tree,
            _ => TreeEntryType::Blob,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Default)]
pub enum TreeEntryMode {
    #[default]
    RegularFile,
    ExecutableFile,
    SymbolicLink,
    Directory,
}

impl From<&str> for TreeEntryMode {
    fn from(value: &str) -> Self {
        match value {
            "100644" => TreeEntryMode::RegularFile,
            "100755" => TreeEntryMode::ExecutableFile,
            "120000" => TreeEntryMode::SymbolicLink,
            "040000" | "40000" => TreeEntryMode::Directory,
            _ => panic!("unknown tree entry mode `{value}`"),
        }
    }
}

impl Display for TreeEntryMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TreeEntryMode::RegularFile => write!(f, "100644"),
            TreeEntryMode::ExecutableFile => write!(f, "100755"),
            TreeEntryMode::SymbolicLink => write!(f, "120000"),
            TreeEntryMode::Directory => write!(f, "040000"),
        }
    }
}

pub(crate) fn build_tree(dot_git_path: &PathBuf, tree_hash: &str) -> anyhow::Result<Tree> {
    let mut tree_entries = vec![];
    let mut object = Object::read(dot_git_path, tree_hash).context("parse out tree object file")?;
    match object.object_type {
        ObjectType::Tree => {
            let mut buffer = Vec::new();
            let mut sha_buffer = [0; 20];

            loop {
                buffer.clear();
                let n = object
                    .reader
                    .read_until(0, &mut buffer)
                    .context("error read until in tree file")?;
                if n == 0 {
                    break;
                }
                object
                    .reader
                    .read_exact(&mut sha_buffer[..])
                    .context("failed to read sha entry")?;

                let header = CStr::from_bytes_with_nul(&buffer)
                    .expect("only one nul at the end")
                    .to_str()
                    .context("tree entry line is no valid UTF-8")?;

                let Some((mode, name)) = header.split_once(' ') else {
                    bail!("invalid tree entry line `{header}`");
                };

                let sha = hex::encode(&sha_buffer);
                tree_entries.push(TreeEntry {
                    mode: mode.into(),
                    name: name.to_string(),
                    sha,
                });
                //   tree <size>\0
                //   <mode> <name>\0<20_byte_sha>
                //   <mode> <name>\0<20_byte_sha>
            }
            // read the remaining bytes to size? or piece meal them?
            Ok(Tree {
                entries: tree_entries,
            })
        }
        _ => bail!("object type '{}' not supported", object.object_type),
    }
}

#[cfg(test)]
mod tests {
    use crate::test::{build_simple_app_git, build_test_git, write_to_git_objects};

    use super::*;

    #[test]
    fn test_build_tree() -> anyhow::Result<()> {
        let git = build_simple_app_git()?;
        let tree_sha = String::from("825ad6339808aa69dd0b2d487586a32fe4b6be17");
        let tree = build_tree(&git.config.dot_git_path, &tree_sha)?;
        assert_eq!(tree.entries.len(), 3);
        assert_eq!(
            tree.entries[0],
            TreeEntry {
                mode: TreeEntryMode::RegularFile,
                name: String::from(".gitignore"),
                sha: String::from("ea8c4bf7f35f6f77f75d92ad8ce8349f6e81ddba"),
            }
        );
        assert_eq!(
            tree.entries[1],
            TreeEntry {
                mode: TreeEntryMode::RegularFile,
                name: String::from("Cargo.toml"),
                sha: String::from("f195397afef8ad7a138507d1cf1c118d6e0d6dfc"),
            }
        );
        assert_eq!(
            tree.entries[2],
            TreeEntry {
                mode: TreeEntryMode::Directory,
                name: String::from("src"),
                sha: String::from("305157a396c6858705a9cb625bab219053264ee4"),
            }
        );
        Ok(())
    }
}
