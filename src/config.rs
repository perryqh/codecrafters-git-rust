#[derive(Debug)]
pub struct Config<W: std::io::Write, X: std::io::Write> {
    pub writer: W,
    pub error_writer: X,
    pub root: std::path::PathBuf,
}

impl Default for Config<std::io::Stdout, std::io::Stderr> {
    fn default() -> Self {
        Self {
            writer: std::io::stdout(),
            error_writer: std::io::stderr(),
            root: std::env::current_dir().unwrap(),
        }
    }
}
