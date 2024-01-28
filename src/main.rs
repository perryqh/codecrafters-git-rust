use std::env;
use std::fs;
use std::io::Read;
use anyhow::Context;
use flate2::read::ZlibDecoder;

fn init() {
    fs::create_dir(".git").unwrap();
    fs::create_dir(".git/objects").unwrap();
    fs::create_dir(".git/refs").unwrap();
    fs::write(".git/HEAD", "ref: refs/heads/master\n").unwrap();
    println!("Initialized git directory")
}

const OBJECT_DIR: &str = ".git/objects/";

fn cat_file(_flag: &str, sha: &str) -> anyhow::Result<()> {
    let path = format!("{}{}/{}", OBJECT_DIR, &sha[..2], &sha[2..]);
    let content = fs::read(path).context("failed to read object")?;
    let mut z = ZlibDecoder::new(&content[..]);
    let mut s = String::new();
    z.read_to_string(&mut s).unwrap();
    print!("{}", &s[8..]);

    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    match &*args[1] {
        "init" => init(),
        "cat-file" => cat_file(&args[2], &args[3]).expect("failed to cat file"),
        _ => println!("unknown command: {}", args[1]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::prelude::*;

    #[test]
    fn test_cat_file() {
        let mut file = File::create("test.txt").unwrap();
        file.write_all(b"test").unwrap();
        let mut file = File::open("test.txt").unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "test");
    }
}
