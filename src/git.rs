use std::{fs, io::{BufRead, Read}, path::PathBuf};

use anyhow::{bail, ensure, Context};

pub struct Git<W: std::io::Write, X: std::io::Write> {
    pub writer: W,
    pub error_writer: X,
    pub root: std::path::PathBuf,
}

impl Default for Git<std::io::Stdout, std::io::Stderr> {
    fn default() -> Self {
        Self {
            writer: std::io::stdout(),
            error_writer: std::io::stderr(),
            root: std::env::current_dir().unwrap(),
        }
    }
}

enum ObjectType {
    Blob,
    // Tree,
    Commit,
    // Tag,
}

impl<W: std::io::Write, X: std::io::Write> Git<W, X> {
    pub fn init(&mut self) -> anyhow::Result<()> {
        fs::create_dir(self.root.join(".git"))?;
        fs::create_dir(self.root.join(".git/objects"))?;
        fs::create_dir(self.root.join(".git/refs"))?;
        fs::write(self.root.join(".git/HEAD"), "ref: refs/heads/master\n")?;

        write!(self.writer, "Initialized git directory\n")?;
        Ok(())
    }

    pub fn hash_object(&mut self, write: &bool, file: &PathBuf) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn cat_file(&mut self, pretty_print: &bool, object_hash: &str) -> anyhow::Result<()> {
        let file = fs::File::open(self.root.join(format!(
            ".git/objects/{}/{}",
            &object_hash[..2],
            &object_hash[2..]
        ))).context("cannot find file for {object_hash}")?;
        let z = flate2::read::ZlibDecoder::new(file);
        let mut z = std::io::BufReader::new(z);
        let mut buffer = Vec::new();
        z.read_until(0, &mut buffer).context("error read until in cat file")?;
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
        let object_type = match kind {
            "blob" => ObjectType::Blob,
            "commit" => ObjectType::Commit,
            _ => bail!("Unknown object type: '{}'", kind),
        };
        let size = size
            .to_string()
            .parse::<u64>()
            .context(".git/objects file header has invalid size: {size}")?;
        let mut z = z.take(size);
        let n = std::io::copy(&mut z, &mut self.writer).context("Failed to write to stdout")?;
        ensure!(
            n == size as u64,
            ".git/object file was not the expected size (expected: {}, actual: {})",
            size,
            n
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use sha1::{Digest, Sha1};
    use tempfile::tempdir;
    use flate2::{write::ZlibEncoder, Compression};
    use flate2::read::ZlibDecoder;

    use super::*;

    #[test]
    fn test_init() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let writer = Vec::new();
        let error_writer = Vec::new();
        let mut git = Git {
            writer,
            error_writer,
            root: temp_dir.path().to_path_buf(),
        };
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
        let result_string = String::from_utf8(git.writer).expect("Found invalid UTF-8");
        assert_eq!(result_string, "Initialized git directory\n");

        Ok(())
    }

    #[test]
    fn test_cat_file() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let file_contents = b"blob 11\0hello world";
        let mut hasher = Sha1::new();
        hasher.update(file_contents);
        let result = hasher.finalize();
        let hash = format!("{:x}", result);

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(file_contents)?;
        let compressed_bytes = encoder.finish()?;
        assert_eq!(compressed_bytes.len(), 27);

        fs::create_dir_all(temp_dir.path().join(".git/objects/").join(&hash[..2]))?;
        let file_path = temp_dir.path().join(".git/objects/").join(&hash[..2]).join(&hash[2..]);
        fs::write(&file_path, compressed_bytes).context("error writing {file_path}")?;

        let file = fs::File::open(&file_path).context("error opening file")?;
        let z = ZlibDecoder::new(file);
        let mut z = std::io::BufReader::new(z);
        let mut buffer = Vec::new();
        z.read_until(0, &mut buffer).context("sanity 33read until in cat file")?;
        dbg!(&buffer);

        let writer = Vec::new();
        let error_writer = Vec::new();
        let mut git = Git {
            writer,
            error_writer,
            root: temp_dir.path().to_path_buf(),
        };
        git.cat_file(&true, &hash).context("unable to cat the file")?;
        let result_string = String::from_utf8(git.writer).expect("Found invalid UTF-8");
        assert_eq!(result_string, "hello world");
        Ok(())
    }
}
