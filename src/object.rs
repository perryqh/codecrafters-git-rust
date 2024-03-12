use anyhow::Context;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::Digest;
use sha1::Sha1;
use std::ffi::CStr;
use std::fmt;
use std::fs;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug)]
pub(crate) struct Object<R> {
    pub(crate) object_type: ObjectType,
    pub(crate) expected_size: u64,
    pub(crate) reader: R,
}

#[derive(Debug)]
pub(crate) enum ObjectType {
    Blob,
    Tree,
    Commit,
    // Tag,
}

#[derive(Debug)]
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

impl fmt::Display for ObjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectType::Blob => write!(f, "blob"),
            ObjectType::Tree => write!(f, "tree"),
            ObjectType::Commit => write!(f, "commit"),
        }
    }
}

impl Object<()> {
    pub(crate) fn blob_from_file(file: impl AsRef<Path>) -> anyhow::Result<Object<impl Read>> {
        let file = file.as_ref();
        let stat = std::fs::metadata(file).with_context(|| format!("stat {}", file.display()))?;
        let file = std::fs::File::open(file).with_context(|| format!("open {}", file.display()))?;

        Ok(Object {
            object_type: ObjectType::Blob,
            expected_size: stat.len(),
            reader: file,
        })
    }

    pub(crate) fn read(root_path: &PathBuf, hash: &str) -> anyhow::Result<Object<impl BufRead>> {
        let f = std::fs::File::open(root_path.join(format!(
            ".git/objects/{}/{}",
            &hash[..2],
            &hash[2..]
        )))
        .context("open in .git/objects")?;
        let z = ZlibDecoder::new(f);
        let mut z = BufReader::new(z);
        let mut buf = Vec::new();
        z.read_until(0, &mut buf)
            .context("read header from .git/objects")?;
        let header = CStr::from_bytes_with_nul(&buf)
            .expect("know there is exactly one nul, and it's at the end");
        let header = header
            .to_str()
            .context(".git/objects file header isn't valid UTF-8")?;
        let Some((object_type, size)) = header.split_once(' ') else {
            anyhow::bail!(".git/objects file header did not start with a known type: '{header}'");
        };
        let object_type: ObjectType = match object_type {
            "blob" => ObjectType::Blob,
            "tree" => ObjectType::Tree,
            "commit" => ObjectType::Commit,
            _ => anyhow::bail!("unknown object_type '{object_type}'"),
        };
        let size = size
            .parse::<u64>()
            .context(".git/objects file header has invalid size: {size}")?;

        let z = z.take(size);
        Ok(Object {
            object_type,
            expected_size: size,
            reader: z,
        })
    }
}

impl<R> Object<R>
where
    R: Read,
{
    pub(crate) fn write(mut self, writer: impl Write) -> anyhow::Result<[u8; 20]> {
        let writer = ZlibEncoder::new(writer, Compression::default());
        let mut writer = HashWriter {
            writer,
            hasher: Sha1::new(),
        };
        write!(writer, "{} {}\0", self.object_type, self.expected_size)?;
        std::io::copy(&mut self.reader, &mut writer).context("stream file into blob")?;
        let _ = writer.writer.finish()?;
        let hash = writer.hasher.finalize();
        Ok(hash.into())
    }

    pub(crate) fn write_to_objects(self, root_path: &PathBuf) -> anyhow::Result<[u8; 20]> {
        let tmp = "temporary";
        let hash = self
            .write(std::fs::File::create(tmp).context("construct temporary file for tree")?)
            .context("stream tree object into tree object file")?;
        let hash_hex = hex::encode(hash);
        fs::create_dir_all(root_path.join(format!(".git/objects/{}/", &hash_hex[..2])))
            .context("create subdir of .git/objects")?;
        fs::rename(
            tmp,
            root_path.join(format!(
                ".git/objects/{}/{}",
                &hash_hex[..2],
                &hash_hex[2..]
            )),
        )
        .context("move tree file into .git/objects")?;
        Ok(hash)
    }
}
