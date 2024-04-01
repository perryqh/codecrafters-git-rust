use std::path::PathBuf;

use clap::command;
use clap::Parser;
use clap::Subcommand;
use git_starter_rust::git::Git;

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
    HashObject {
        #[clap(short = 'w')]
        write: bool,
        file: PathBuf,
    },
    LsTree {
        #[clap(short = 'n', long)]
        name_only: bool,
        #[clap(name = "tree-hash")]
        tree_hash: String,
    },
    WriteTree,
    CommitTree {
        #[clap(short = 'm')]
        message: String,
        #[clap(short = 'p')]
        parent_hash: Option<String>,
        tree_hash: String,
    },
    Commit {
        #[clap(short = 'm')]
        message: String,
    },
}

fn main() -> anyhow::Result<()> {
    let mut git = Git {
        config: Default::default(),
    };
    let args = Args::parse();
    match args.command {
        Command::Init => git.init(),
        Command::CatFile {
            pretty_print,
            object_hash,
        } => git.cat_file(&pretty_print, &object_hash),
        Command::HashObject { write, file } => git.hash_object(&write, &file),
        Command::LsTree {
            name_only,
            tree_hash,
        } => git.ls_tree(&name_only, &tree_hash),
        Command::WriteTree => git.write_tree(),
        Command::CommitTree {
            message,
            tree_hash,
            parent_hash,
        } => git.commit_tree(&message, &tree_hash, parent_hash),
        Command::Commit { message } => git.commit(&message),
    }
}
