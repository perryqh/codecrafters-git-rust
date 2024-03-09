use std::fs;
use clap::command;
use clap::Parser;
use clap::Subcommand;

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
}

fn main() {
 let args = Args::parse();
    match args.command {
        Command::Init => init(),
    }
}

fn init() {
    fs::create_dir(".git").unwrap();
    fs::create_dir(".git/objects").unwrap();
    fs::create_dir(".git/refs").unwrap();
    fs::write(".git/HEAD", "ref: refs/heads/master\n").unwrap();
    println!("Initialized git directory")
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cat_file() {
       
    }
}
