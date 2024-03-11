use std::{
    fs::{self, metadata, File},
    io::{copy, BufRead, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{bail, ensure, Context};
use flate2::write::ZlibEncoder;
use sha1::{Digest, Sha1};

pub struct Git<W: std::io::Write, X: std::io::Write> {
    pub writer: W,
    pub error_writer: X,
    pub root: std::path::PathBuf,
}

struct HashWriter<W: std::io::Write> {
    writer: W,
    hasher: Sha1,
}

impl<W> std::io::Write for HashWriter<W>
where
    W: std::io::Write,
{
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.writer.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
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
        fn write_blob<W>(file: &Path, writer: W) -> anyhow::Result<String>
        where
            W: Write,
        {
            let stat = metadata(file).context("cannot stat file")?;
            let writer = ZlibEncoder::new(writer, flate2::Compression::default());
            let mut writer = HashWriter {
                writer,
                hasher: Sha1::new(),
            };
            write!(writer, "blob {}\0", stat.len())?;
            let mut file = File::open(file).context("cannot open file")?;
            copy(&mut file, &mut writer).context("stream file into blob")?;
            let _ = writer.writer.finish()?;
            let hash = writer.hasher.finalize();
            Ok(hex::encode(hash))
        }
        let hash = if *write {
            let tmp = "temporary";
            let hash = write_blob(
                &file,
                File::create(tmp).context("cannot create temporary file")?,
            )
            .context("cannot write blob")?;
            fs::create_dir_all(self.root.join(".git/objects").join(&hash[..2]))?;
            fs::rename(
                tmp,
                self.root
                    .join(".git/objects")
                    .join(&hash[..2])
                    .join(&hash[2..]),
            )?;
            hash
        } else {
            write_blob(&file, std::io::sink()).context("cannot write blob to sink")?
        };
        write!(self.writer, "{hash}", hash = hash)?;
        Ok(())
    }

    pub fn cat_file(&mut self, pretty_print: &bool, object_hash: &str) -> anyhow::Result<()> {
        let file = fs::File::open(self.root.join(format!(
            ".git/objects/{}/{}",
            &object_hash[..2],
            &object_hash[2..]
        )))
        .context("cannot find file for {object_hash}")?;
        let z = flate2::read::ZlibDecoder::new(file);
        let mut z = std::io::BufReader::new(z);
        let mut buffer = Vec::new();
        z.read_until(0, &mut buffer)
            .context("error read until in cat file")?;
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
        let _object_type = match kind {
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

    use flate2::read::ZlibDecoder;
    use flate2::{write::ZlibEncoder, Compression};
    use sha1::{Digest, Sha1};
    use tempfile::tempdir;

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
        let file_path = temp_dir
            .path()
            .join(".git/objects/")
            .join(&hash[..2])
            .join(&hash[2..]);
        fs::write(&file_path, compressed_bytes).context("error writing {file_path}")?;

        let file = fs::File::open(&file_path).context("error opening file")?;
        let z = ZlibDecoder::new(file);
        let mut z = std::io::BufReader::new(z);
        let mut buffer = Vec::new();
        z.read_until(0, &mut buffer)
            .context("sanity 33read until in cat file")?;
        dbg!(&buffer);

        let writer = Vec::new();
        let error_writer = Vec::new();
        let mut git = Git {
            writer,
            error_writer,
            root: temp_dir.path().to_path_buf(),
        };
        git.cat_file(&true, &hash)
            .context("unable to cat the file")?;
        let result_string = String::from_utf8(git.writer).expect("Found invalid UTF-8");
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
        let mut git = Git {
            writer,
            error_writer,
            root: temp_dir.path().to_path_buf(),
        };
        git.hash_object(&true, &tmp_file_path)?;
        let hash = String::from_utf8(git.writer).expect("Found invalid UTF-8");
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
}

// https://youtu.be/u0VotuGzD_w?t=5935
