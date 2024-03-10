use anyhow::bail;
use anyhow::ensure;
use anyhow::Context;
use clap::command;
use clap::Parser;
use clap::Subcommand;
use flate2::read::ZlibDecoder;
use std::ffi::CStr;
use std::fs;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand, Debug)]
#[command(about, long_about = None)]
enum Command {
    Init,
    CatFile {
        #[clap(short = 'p', long)]
        pretty_print: bool,

        #[clap(name = "object-hash")]
        object_hash: String,
    },
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Init => init(),
        Command::CatFile {
            pretty_print,
            object_hash,
        } => cat_file(&pretty_print, &object_hash),
    }
}

enum ObjectType {
    Blob,
    // Tree,
    Commit,
    // Tag,
}

fn cat_file(pretty_print: &bool, object_hash: &String) -> anyhow::Result<()> {
    let file = File::open(format!(
        ".git/objects/{}/{}",
        &object_hash[..2],
        &object_hash[2..]
    ))
    .context("Failed to open object file")?;

    let z = ZlibDecoder::new(file);
    let mut z = BufReader::new(z);
    let mut buffer = Vec::new();
    z.read_until(0, &mut buffer)?;
    let header = CStr::from_bytes_with_nul(&buffer)
        .context("Failed to read bytes with nul")?
        .to_str()
        .context("Failed to parse object header")?;
    let Some((kind, size)) = header.split_once(' ') else {
        bail!(".git/objects file header did not start with a knonw type: '{}'", header);
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

    //match object_type {
      //  ObjectType::Blob => {
           let stdout = std::io::stdout();
           let mut stdout = stdout.lock();
           let n = std::io::copy(&mut z, &mut stdout).context("Failed to write to stdout")?;
           ensure!(n == size as u64, ".git/object file was not the expected size (expected: {}, actual: {})", size, n);
       // }
    //}
    Ok(())
}

fn init() -> anyhow::Result<()> {
    fs::create_dir(".git").unwrap();
    fs::create_dir(".git/objects").unwrap();
    fs::create_dir(".git/refs").unwrap();
    fs::write(".git/HEAD", "ref: refs/heads/master\n").unwrap();
    println!("Initialized git directory");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;
    use std::io::{Read, Write};

    use anyhow::ensure;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    use super::*;

    #[test]
    fn test_cat_file() {}

    #[test]
    fn test_read_write_object() -> anyhow::Result<()> {
        let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
        e.write_all(b"blob 11")?;
        e.write_all(&[0])?;
        e.write_all(b"hello world")?;
        let compressed_bytes = e.finish()?;
        //let mut temp_file = tempfile()?;
        //temp_file.write(&compressed_bytes)?;

        //let z = ZlibDecoder::new(temp_file);
        let z = ZlibDecoder::new(&compressed_bytes[..]);
        let mut z = BufReader::new(z);
        let mut buffer = Vec::new();
        z.read_until(0, &mut buffer)?;
        let header = CStr::from_bytes_with_nul(&buffer)
            .context("Failed to read bytes with nul")?
            .to_str()
            .context("Failed to parse object header")?;

        let Some(size) = header.strip_prefix("blob ") else {
            bail!(".git/objects file header did not start with 'blob'");
        };
        let size = size
            .parse::<usize>()
            .context(format!("Failed to parse object size: '{}'", &size))?;
        buffer.clear();
        buffer.resize(size, 0);
        z.read_exact(buffer.as_mut_slice())
            .context("Failed to read contents of object file")?;

        let n = z.read(&mut []).context("Expected end of file")?;
        ensure!(n == 0, "object had trailing bytes");
        assert_eq!(buffer, b"hello world");
        println!("buffer: {:?}", buffer);
        Ok(())
    }
}
