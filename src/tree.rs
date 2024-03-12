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
            "040000" => TreeEntryMode::Directory,
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

pub(crate) fn build_tree(root_path: &PathBuf, tree_hash: &str) -> anyhow::Result<Tree> {
    let mut tree_entries = vec![];
    let mut object = Object::read(root_path, tree_hash).context("parse out tree object file")?;
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
    use crate::test::{build_test_git, write_to_git_objects};

    use super::*;

    #[test]
    fn test_build_tree() -> anyhow::Result<()> {
        // 100644 blob abafc304b7280dac41f0949acc30eeb6a7a70eb4	README.md
        // 040000 tree dc521eaed6e6b7ba3513b32713539d1fe44c5a26	ai-assistant
        // 040000 tree 12ce3a605dcfd1cd80cae6b1df63ed29ac44a25b	app-apis
        // 040000 tree 18caae42a9b3147a3d9083631b5d7ca9022cbf91	app-benefits-apis
        //
        //   tree <size>\0
        //   <mode> <name>\0<20_byte_sha>
        //   <mode> <name>\0<20_byte_sha>
        let e1: Vec<u8> = [b"0100644 README.md\0", &hex::decode(b"abafc304b7280dac41f0949acc30eeb6a7a70eb4")?[..]].concat();
        let file_contents = b"tree 239\0100644 README.md\0abafc304b7280dac41f0949acc30eeb6a7a70eb4040000 ai-assistant\0dc521eaed6e6b7ba3513b32713539d1fe44c5a26040000 app-apis\012ce3a605dcfd1cd80cae6b1df63ed29ac44a25b040000 app-benefits-apis\018caae42a9b3147a3d9083631b5d7ca9022cbf91";
        let mut git = build_test_git()?;
        let (tree_sha, _file_path) = write_to_git_objects(&git, file_contents)?;
        let tree = build_tree(&git.config.root, &tree_sha)?;
        assert_eq!(tree.entries.len(), 4);
        assert_eq!(
            tree.entries[0],
            TreeEntry {
                mode: TreeEntryMode::RegularFile,
                name: String::from("README.md"),
                sha: String::from("abafc304b7280dac41f0949acc30eeb6a7a70eb4"),
            }
        );
        assert_eq!(
            tree.entries[1],
            TreeEntry {
                mode: TreeEntryMode::Directory,
                name: String::from("ai-assistant"),
                sha: String::from("dc521eaed6e6b7ba3513b32713539d1fe44c5a26"),
            }
        );
        assert_eq!(
            tree.entries[2],
            TreeEntry {
                mode: TreeEntryMode::Directory,
                name: String::from("ai-assistant"),
                sha: String::from("12ce3a605dcfd1cd80cae6b1df63ed29ac44a25b"),
            }
        );
        assert_eq!(
            tree.entries[3],
            TreeEntry {
                mode: TreeEntryMode::Directory,
                name: String::from("ai-assistant"),
                sha: String::from("18caae42a9b3147a3d9083631b5d7ca9022cbf91"),
            }
        );
        Ok(())
    }
}
