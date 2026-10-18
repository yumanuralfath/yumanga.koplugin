#[cfg(feature = "aidoku-runtime")]
mod aidoku_runtime;
mod cli;
mod download;
mod models;
mod sources;
mod util;

fn main() -> std::process::ExitCode {
    cli::run()
}
