use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, ensure, Context};

use crate::{
    config::Config,
    object::{Object, ObjectType},
    tree::{build_tree, write_tree_for},
};
#[derive(Debug)]
pub struct Git<W: std::io::Write, X: std::io::Write> {
    pub config: Config<W, X>,
}

impl<W: std::io::Write, X: std::io::Write> Git<W, X> {
    pub fn init(&mut self) -> anyhow::Result<()> {
        fs::create_dir(&self.config.dot_git_path)?;
        fs::create_dir(self.config.dot_git_path.join("objects"))?;
        fs::create_dir(self.config.dot_git_path.join("refs"))?;
        fs::write(
            self.config.dot_git_path.join("HEAD"),
            "ref: refs/heads/master\n",
        )?;

        writeln!(self.config.writer, "Initialized git directory")?;
        Ok(())
    }

    pub fn hash_object(&mut self, write: &bool, file: &PathBuf) -> anyhow::Result<()> {
        let object = Object::blob_from_file(file).context("open blob input file")?;
        let hash = if *write {
            object
                .write_to_objects(&self.config.dot_git_path)
                .context("stream file into blob object file")?
        } else {
            object
                .write(std::io::sink())
                .context("stream file into blob object")?
        };

        write!(self.config.writer, "{}", hex::encode(hash))?;

        Ok(())
    }

    pub fn cat_file(&mut self, _pretty_print: &bool, object_hash: &str) -> anyhow::Result<()> {
        let mut object = Object::read(&self.config.dot_git_path, object_hash)
            .context("parse out blob object file")?;

        match object.object_type {
            ObjectType::Blob => {
                let n = std::io::copy(&mut object.reader, &mut self.config.writer)
                    .context("Failed to write to stdout")?;
                ensure!(
                    n == object.expected_size,
                    ".git/object file was not the expected size (expected: {}, actual: {})",
                    object.expected_size,
                    n
                );
            }
            _ => bail!("object type not supported"),
        }
        Ok(())
    }

    pub fn ls_tree(&mut self, name_only: &bool, tree_sha: &str) -> anyhow::Result<()> {
        let tree = build_tree(&self.config.dot_git_path, tree_sha)?;
        for entry in tree.entries {
            if *name_only {
                writeln!(self.config.writer, "{}", &entry.name)?;
            } else {
                writeln!(
                    self.config.writer,
                    "{} {} {}\t{}",
                    entry.mode,
                    entry.tree_entry_type(),
                    entry.sha,
                    entry.name
                )?;
            }
        }
        Ok(())
    }

    pub fn write_tree(&mut self) -> anyhow::Result<()> {
        let Some(hash) = write_tree_for(&self.config.dot_git_path, Path::new("."))
            .context("construct root tree object")?
        else {
            anyhow::bail!("failed to write tree");
        };

        writeln!(self.config.writer, "{}", hex::encode(hash))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{
        build_git_from_fixture, build_simple_app_git, build_test_git, write_to_git_objects,
    };
    use flate2::read::ZlibDecoder;
    use std::io::{BufRead, Read, Write};
    use tempfile::tempdir;

    #[test]
    fn test_init() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let writer = Vec::new();
        let error_writer = Vec::new();
        let config = Config {
            writer,
            error_writer,
            dot_git_path: temp_dir.path().to_path_buf().join(".git"),
        };
        let mut git = Git { config };
        git.init()?;
        let git_dir = temp_dir.path().join(".git");
        assert!(git_dir.exists());
        let objects_dir = git_dir.join("objects");
        assert!(objects_dir.exists());
        let refs_dir = git_dir.join("refs");
        assert!(refs_dir.exists());
        let head_file = git_dir.join("HEAD");
        assert!(head_file.exists());
        let head_contents = std::fs::read_to_string(head_file)?;
        assert_eq!(head_contents, "ref: refs/heads/master\n");
        let result_string = String::from_utf8(git.config.writer).expect("Found invalid UTF-8");
        assert_eq!(result_string, "Initialized git directory\n");

        Ok(())
    }

    #[test]
    fn test_cat_file() -> anyhow::Result<()> {
        let file_contents = b"blob 11\0hello world";
        let git = build_test_git()?;
        let (hash, file_path) = write_to_git_objects(&git, file_contents)?;

        let file = fs::File::open(&file_path).context("error opening file")?;
        let z = ZlibDecoder::new(file);
        let mut z = std::io::BufReader::new(z);
        let mut buffer = Vec::new();
        z.read_until(0, &mut buffer)
            .context("sanity 33read until in cat file")?;
        dbg!(&buffer);

        let writer = Vec::new();
        let error_writer = Vec::new();
        let config = Config {
            writer,
            error_writer,
            dot_git_path: git.config.dot_git_path.as_path().to_path_buf(),
        };
        let mut git = Git { config };
        git.cat_file(&true, &hash)
            .context("unable to cat the file")?;
        let result_string = String::from_utf8(git.config.writer).expect("Found invalid UTF-8");
        assert_eq!(result_string, "hello world");
        Ok(())
    }

    #[test]
    fn test_hash_object() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let file_contents = b"hello world with some content";
        let mut tmp_file = tempfile::NamedTempFile::new()?;
        let tmp_file_path = tmp_file.path().to_path_buf();
        tmp_file.write_all(file_contents)?;

        let writer = Vec::new();
        let error_writer = Vec::new();
        let config = Config {
            writer,
            error_writer,
            dot_git_path: temp_dir.path().to_path_buf().join(".git"),
        };
        fs::create_dir(&config.dot_git_path)?;
        let mut git = Git { config };
        git.hash_object(&true, &tmp_file_path)?;
        let hash = String::from_utf8(git.config.writer).expect("Found invalid UTF-8");
        let object_path = temp_dir
            .path()
            .join(".git/objects/")
            .join(&hash[..2])
            .join(&hash[2..]);
        assert!(object_path.exists());

        let file = fs::File::open(&object_path).context("error opening file")?;
        let z = ZlibDecoder::new(file);
        let mut z = std::io::BufReader::new(z);
        let mut buffer = Vec::new();
        z.read_until(0, &mut buffer)
            .context("read until in cat file")?;
        let header = std::ffi::CStr::from_bytes_with_nul(&buffer)
            .context("Failed to read bytes with nul")?
            .to_str()
            .context("Failed to parse object header")?;
        let Some((kind, size)) = header.split_once(' ') else {
            bail!(
                ".git/objects file header did not start with a knonw type: '{}'",
                header
            );
        };
        assert_eq!(kind, "blob");
        assert_eq!(size, "29");
        let mut z = z.take(29);
        let mut content = Vec::new();
        z.read_to_end(&mut content)
            .context("Failed to read to end of file")?;
        assert_eq!(content, file_contents);

        Ok(())
    }

    #[test]
    fn test_ls_tree_name_only() -> anyhow::Result<()> {
        let mut git = build_simple_app_git()?;
        let tree_sha = "825ad6339808aa69dd0b2d487586a32fe4b6be17";
        git.ls_tree(&true, tree_sha)?;
        let result_string = String::from_utf8(git.config.writer).expect("Found invalid UTF-8");
        assert_eq!(result_string, String::from(".gitignore\nCargo.toml\nsrc\n"));
        Ok(())
    }

    #[test]
    fn test_ls_tree() -> anyhow::Result<()> {
        let mut git = build_simple_app_git()?;
        let tree_sha = "825ad6339808aa69dd0b2d487586a32fe4b6be17";
        git.ls_tree(&false, tree_sha)?;
        let actual = String::from_utf8(git.config.writer).expect("Found invalid UTF-8");
        let expected = "100644 blob ea8c4bf7f35f6f77f75d92ad8ce8349f6e81ddba	.gitignore
100644 blob f195397afef8ad7a138507d1cf1c118d6e0d6dfc	Cargo.toml
040000 tree 305157a396c6858705a9cb625bab219053264ee4	src
"
        .to_string();
        assert_eq!(actual, expected);
        Ok(())
    }
}
