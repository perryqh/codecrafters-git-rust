use std::fmt::Write;
use std::{
    cmp::Ordering,
    ffi::CStr,
    fmt::{self, Display},
    fs::{self, Metadata},
    io::{BufRead, Cursor, Read},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
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

#[derive(Debug, PartialEq, Eq, Default, Clone, Copy)]
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

impl From<&Metadata> for TreeEntryMode {
    fn from(meta: &Metadata) -> Self {
        if meta.is_dir() {
            TreeEntryMode::Directory
        } else if meta.is_symlink() {
            TreeEntryMode::SymbolicLink
        } else if (meta.permissions().mode() & 0o111) != 0 {
            TreeEntryMode::ExecutableFile
        } else {
            TreeEntryMode::RegularFile
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

#[derive(Debug, Default)]
struct TreeEntryBytesBuilder {
    mode: Option<TreeEntryMode>,
    name: Option<String>,
    path: Option<PathBuf>,
    is_dir: Option<bool>,
}

impl TreeEntryBytesBuilder {
    fn dir_entry(mut self, entry: &fs::DirEntry) -> Self {
        let meta = entry.metadata().expect("metadata for directory entry");
        self.mode = Some(TreeEntryMode::from(&meta));
        self.name = Some(entry.file_name().to_string_lossy().to_string());
        self.path = Some(entry.path());
        self.is_dir = Some(meta.is_dir());
        self
    }

    fn path(&self) -> &Path {
        self.path.as_ref().expect("path is required")
    }

    fn is_dir(&self) -> bool {
        self.is_dir.expect("is_dir is required")
    }

    fn is_dot_git_entry(&self, dot_git_path: &Path) -> bool {
        self.name.as_ref().map_or(false, |name| {
            dot_git_path
                .file_name()
                .and_then(|file_name| file_name.to_str())
                .map_or(false, |file_name_str| name == file_name_str)
        })
    }

    fn to_raw_bytes(&self, hash: [u8; 20]) -> anyhow::Result<Vec<u8>> {
        let mode = self.mode.context("mode is required")?;
        let mode_string = match mode {
            TreeEntryMode::Directory => "40000".to_string(), // git doesn't use 040000
            _ => format!("{}", mode),
        };
        let file_name = self.name.as_ref().context("name is required")?;
        let mut raw_bytes = Vec::new();
        raw_bytes.extend(mode_string.as_bytes());
        raw_bytes.push(b' ');
        raw_bytes.extend(file_name.as_bytes());
        raw_bytes.push(0);
        raw_bytes.extend(hash);
        Ok(raw_bytes)
    }
}

fn compare_tree_entry_bytes_builder(
    a: &TreeEntryBytesBuilder,
    b: &TreeEntryBytesBuilder,
) -> Ordering {
    let afn = a.name.as_ref().expect("name is required");
    let afn = afn.as_bytes();
    let bfn = b.name.as_ref().expect("name is required");
    let bfn = bfn.as_bytes();
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
    } else if a.is_dir() {
        Some(b'/')
    } else {
        None
    };
    let c2 = if let Some(c) = bfn.get(common_len).copied() {
        Some(c)
    } else if b.is_dir() {
        Some(b'/')
    } else {
        None
    };

    c1.cmp(&c2)
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
    let dir = fs::read_dir(path).with_context(|| format!("open directory {}", path.display()))?;

    let mut entries = Vec::new();
    for entry in dir {
        let entry = entry.with_context(|| format!("bad directory entry in {}", path.display()))?;
        let builder = TreeEntryBytesBuilder::default().dir_entry(&entry);
        entries.push(builder);
    }

    entries.sort_unstable_by(compare_tree_entry_bytes_builder);

    let mut tree_object = Vec::new();
    for builder in entries {
        if builder.is_dot_git_entry(dot_git_path) {
            continue;
        }

        let hash = if builder.is_dir() {
            let Some(hash) = write_tree_for(dot_git_path, builder.path())? else {
                // empty directory, so don't include in parent
                continue;
            };
            hash
        } else {
            Object::blob_from_file(&builder.path())
                .context("open blob input file")?
                .write_to_objects(dot_git_path)
                .context("writing to objects")?
        };
        tree_object.extend(builder.to_raw_bytes(hash)?);
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

pub(crate) fn commit_tree(
    dot_git_path: &Path,
    message: &str,
    tree_hash: &str,
    parent_hash: Option<&str>,
) -> anyhow::Result<Option<[u8; 20]>> {
    let mut commit = String::new();
    writeln!(commit, "tree {tree_hash}")?;
    if let Some(parent_hash) = parent_hash {
        writeln!(commit, "parent {parent_hash}")?;
    }
    let time = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .context("current system time is before UNIX epoch")?;
    writeln!(
        commit,
        "author Perry Hertler <perry@hertler.org> {} +0000",
        time.as_secs()
    )?;
    writeln!(
        commit,
        "committer Perry Hertler <perry@hertler.org> {} +0000",
        time.as_secs()
    )?;
    writeln!(commit, "")?;
    writeln!(commit, "{message}")?;
    Ok(Some(
        Object {
            object_type: ObjectType::Commit,
            expected_size: commit.len() as u64,
            reader: Cursor::new(commit),
        }
        .write_to_objects(dot_git_path)
        .context("write commit object")?,
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::tempdir;

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

    #[test]
    fn test_write_tree_with_one_file() -> anyhow::Result<()> {
        let tmp_dir = tempdir()?;
        let dot_git = tmp_dir.path().join("dot-git");
        fs::create_dir_all(dot_git.join("objects")).context("create subdir of .git/objects")?;
        let staging_git_dir = PathBuf::from(format!("tests/fixtures/one-file-app"));
        let result = write_tree_for(&dot_git, staging_git_dir.as_path());
        assert!(&result.is_ok());
        assert!(result.as_ref().unwrap().is_some());
        let actual_sha = result?;
        let actual_sha = actual_sha.as_ref().expect("SHA should be present");
        let actual_sha = hex::encode(actual_sha);
        assert_eq!(actual_sha, "5da554cc6d31c65185d6d63ae707cc1328eeb8c2");
        assert_eq!(fs::read_dir(tmp_dir.path().join("dot-git"))?.count(), 1);
        assert_eq!(fs::read_dir(dot_git.join("objects/"))?.count(), 2);
        let tree = build_tree(dot_git.as_path(), &actual_sha)?;
        assert_eq!(tree.entries.len(), 1);
        assert_eq!(
            tree.entries[0],
            TreeEntry {
                mode: TreeEntryMode::RegularFile,
                name: String::from("foo.rs"),
                sha: String::from("3524658cc82dda8611f51bd132493e711d50bb81"),
            }
        );
        Ok(())
    }

    #[test]
    fn test_write_tree_complex() -> anyhow::Result<()> {
        let tmp_dir = tempdir()?;
        let dot_git = tmp_dir.path().join("dot-git");
        fs::create_dir_all(dot_git.join("objects")).context("create subdir of .git/objects")?;
        let staging_git_dir = PathBuf::from(format!("tests/fixtures/complex-app"));
        let result = write_tree_for(&dot_git, staging_git_dir.as_path());
        assert!(&result.is_ok());
        assert!(result.as_ref().unwrap().is_some());
        let actual_sha = result?;
        let actual_sha = actual_sha.as_ref().expect("SHA should be present");
        let actual_sha = hex::encode(actual_sha);
        assert_eq!(actual_sha, "f33421767929a06951899aa91cc699df29c3893b");
        assert_eq!(fs::read_dir(tmp_dir.path().join("dot-git"))?.count(), 1);
        assert_eq!(fs::read_dir(dot_git.join("objects/"))?.count(), 6);
        let tree = build_tree(dot_git.as_path(), &actual_sha)?;
        assert_eq!(tree.entries.len(), 1);
        assert_eq!(
            tree.entries[0],
            TreeEntry {
                mode: TreeEntryMode::Directory,
                name: String::from("src"),
                sha: String::from("32692fb2462bbe82c8b88e54ec5f0fec3badbe88"),
            }
        );
        let tree = build_tree(
            dot_git.as_path(),
            "32692fb2462bbe82c8b88e54ec5f0fec3badbe88",
        )?;
        assert_eq!(tree.entries.len(), 2);
        assert_eq!(
            tree.entries[0],
            TreeEntry {
                mode: TreeEntryMode::RegularFile,
                name: String::from("foo.txt"),
                sha: String::from("1657a67183cbc4719b4818685a2f5635bf481094"),
            }
        );
        assert_eq!(
            tree.entries[1],
            TreeEntry {
                mode: TreeEntryMode::Directory,
                name: String::from("foo"),
                sha: String::from("68ab77c490a5cbdcf70a2094ce43780c5780ab4b"),
            }
        );
        let tree = build_tree(
            dot_git.as_path(),
            "68ab77c490a5cbdcf70a2094ce43780c5780ab4b",
        )?;
        assert_eq!(tree.entries.len(), 2);
        assert_eq!(
            tree.entries[0],
            TreeEntry {
                mode: TreeEntryMode::RegularFile,
                name: String::from("ab.txt"),
                sha: String::from("4a3055de6ce49aa356dd07a1e7feeff79bd18fb8"),
            }
        );
        assert_eq!(
            tree.entries[1],
            TreeEntry {
                mode: TreeEntryMode::RegularFile,
                name: String::from("bc.txt"),
                sha: String::from("2ce489f987a7d6dbfa73a54df760cdc90e841794"),
            }
        );
        Ok(())
    }

    #[test]
    fn test_commit_tree_complex() -> anyhow::Result<()> {
        let tmp_dir = tempdir()?;
        let dot_git = tmp_dir.path().join("dot-git");
        fs::create_dir_all(dot_git.join("objects")).context("create subdir of .git/objects")?;
        let staging_git_dir = PathBuf::from(format!("tests/fixtures/complex-app"));
        let result = write_tree_for(&dot_git, staging_git_dir.as_path());
        assert!(&result.is_ok());
        let tree_sha = hex::encode(result.unwrap().unwrap());
        assert_eq!(tree_sha, "f33421767929a06951899aa91cc699df29c3893b");
        assert_eq!(fs::read_dir(tmp_dir.path().join("dot-git"))?.count(), 1);
        let result = commit_tree(&dot_git, "initial commit", &tree_sha, None)?;
        let commit_sha = hex::encode(result.unwrap());
        assert_eq!(commit_sha.len(), 40);

        let tree = build_tree(dot_git.as_path(), &tree_sha)?;
        assert_eq!(tree.entries.len(), 1);
        assert_eq!(
            tree.entries[0],
            TreeEntry {
                mode: TreeEntryMode::Directory,
                name: String::from("src"),
                sha: String::from("32692fb2462bbe82c8b88e54ec5f0fec3badbe88"),
            }
        );

        Ok(())
    }
}
