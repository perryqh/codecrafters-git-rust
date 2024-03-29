use std::{
    cmp::Ordering,
    ffi::CStr,
    fmt::{self, Display},
    fs,
    io::{BufRead, Cursor, Read},
    os::unix::fs::PermissionsExt,
    path::Path,
};

use anyhow::{bail, Context};

use crate::object::{Object, ObjectType};

#[derive(Debug, PartialEq, Eq, Default)]
pub enum TreeEntryType {
    Blob,
    #[default]
    Tree,
}

impl Display for TreeEntryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TreeEntryType::Blob => write!(f, "blob"),
            TreeEntryType::Tree => write!(f, "tree"),
        }
    }
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

pub(crate) fn build_tree(dot_git_path: &Path, tree_hash: &str) -> anyhow::Result<Tree> {
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

                let sha = hex::encode(sha_buffer);
                tree_entries.push(TreeEntry {
                    mode: mode.into(),
                    name: name.to_string(),
                    sha,
                });
            }
            Ok(Tree {
                entries: tree_entries,
            })
        }
        _ => bail!("object type '{}' not supported", object.object_type),
    }
}
pub(crate) fn write_tree_for(dot_git_path: &Path, path: &Path) -> anyhow::Result<Option<[u8; 20]>> {
    dbg!(path);
    let mut dir =
        fs::read_dir(path).with_context(|| format!("open directory {}", path.display()))?;

    let mut entries = Vec::new();
    while let Some(entry) = dir.next() {
        let entry = entry.with_context(|| format!("bad directory entry in {}", path.display()))?;
        let name = entry.file_name();
        let meta = entry.metadata().context("metadata for directory entry")?;
        entries.push((entry, name, meta));
    }
    entries.sort_unstable_by(|a, b| {
        // git has very specific rules for how to compare names
        // https://github.com/git/git/blob/e09f1254c54329773904fe25d7c545a1fb4fa920/tree.c#L99
        let afn = &a.1;
        let afn_string = afn.to_string_lossy();
        let afn = afn_string.as_bytes();
        let bfn = &b.1;
        let bfn_string = bfn.to_string_lossy();
        let bfn = bfn_string.as_bytes();
        let common_len = std::cmp::min(afn.len(), bfn.len());
        match afn[..common_len].cmp(&bfn[..common_len]) {
            Ordering::Equal => {}
            o => return o,
        }
        if afn.len() == bfn.len() {
            return Ordering::Equal;
        }
        let c1 = if let Some(c) = afn.get(common_len).copied() {
            Some(c)
        } else if a.2.is_dir() {
            Some(b'/')
        } else {
            None
        };
        let c2 = if let Some(c) = bfn.get(common_len).copied() {
            Some(c)
        } else if b.2.is_dir() {
            Some(b'/')
        } else {
            None
        };

        c1.cmp(&c2)
    });
    let mut tree_object = Vec::new();
    for (entry, file_name, meta) in entries {
        if file_name == dot_git_path.file_name().unwrap() {
            continue;
        }
        let mode = if meta.is_dir() {
            "40000"
        } else if meta.is_symlink() {
            "120000"
        } else if (meta.permissions().mode() & 0o111) != 0 {
            // has at least one executable bit set
            "100755"
        } else {
            "100644"
        };
        let path = entry.path();
        let hash = if meta.is_dir() {
            let Some(hash) = write_tree_for(dot_git_path, &path)? else {
                // empty directory, so don't include in parent
                continue;
            };
            hash
        } else {
            let tempfile = tempfile::NamedTempFile::new().context("create temporary file")?;
            let hash = Object::blob_from_file(&path)
                .context("open blob input file")?
                .write(&tempfile)
                .context("stream file into blob")?;
            let hash_hex = hex::encode(hash);
            fs::create_dir_all(dot_git_path.join(format!("objects/{}/", &hash_hex[..2])))
                .context("create subdir of .git/objects")?;
            std::fs::rename(
                tempfile.path(),
                dot_git_path.join(format!("objects/{}/{}", &hash_hex[..2], &hash_hex[2..])),
            )
            .context("move blob file into .git/objects")?;
            hash
        };
        tree_object.extend(mode.as_bytes());
        tree_object.push(b' ');
        tree_object.extend(file_name.to_string_lossy().as_bytes());
        tree_object.push(0);
        tree_object.extend(hash);
    }

    if tree_object.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            Object {
                object_type: ObjectType::Tree,
                expected_size: tree_object.len() as u64,
                reader: Cursor::new(tree_object),
            }
            .write_to_objects(dot_git_path)
            .context("write tree object")?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::test::build_simple_app_git;

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
