mod api;
mod commands;
mod config;
mod repo;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "clearmesh")]
#[command(
    version,
    about = "Encrypted Git-like collaboration for large private files."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Login,
    Logout,
    Auth(commands::auth::AuthCommand),
    Config(commands::auth::ConfigCommand),
    Org(commands::org::OrgCommand),
    Repo(commands::repo::RepoCommand),
    Init,
    Status,
    Commit(commands::commit::CommitArgs),
    Push(commands::sync::KeyArgs),
    Clone(commands::sync::CloneArgs),
    Sync(commands::sync::KeyArgs),
    Log,
    Branch(commands::sync::BranchCommand),
    Merge(commands::merge::MergeArgs),
    Conflicts,
    Checkout(commands::merge::CheckoutArgs),
    Resolve { path: std::path::PathBuf },
    Mount(commands::mount::MountArgs),
    Unmount(commands::mount::UnmountArgs),
    Vfs,
    Vault(commands::vault::VaultCommand),
    Doctor,
    Version,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Login => commands::auth::login().await,
        Command::Logout => commands::auth::logout().await,
        Command::Auth(command) => commands::auth::run_auth(command).await,
        Command::Config(command) => commands::auth::run_config(command).await,
        Command::Org(command) => commands::org::run(command).await,
        Command::Repo(command) => commands::repo::run(command).await,
        Command::Init => commands::commit::init(),
        Command::Status => commands::commit::status(),
        Command::Commit(args) => commands::commit::commit(args).await,
        Command::Push(args) => commands::sync::push(args).await,
        Command::Clone(args) => commands::sync::clone_repo(args).await,
        Command::Sync(args) => commands::sync::sync(args).await,
        Command::Log => commands::sync::log(),
        Command::Branch(command) => commands::sync::branch(command).await,
        Command::Merge(args) => commands::merge::merge(args),
        Command::Conflicts => commands::merge::conflicts(),
        Command::Checkout(args) => commands::merge::checkout(args),
        Command::Resolve { path } => commands::merge::resolve(path),
        Command::Mount(args) => commands::mount::mount(args).await,
        Command::Unmount(args) => commands::mount::unmount(args),
        Command::Vfs => commands::mount::vfs(),
        Command::Vault(command) => commands::vault::run(command).await,
        Command::Doctor => commands::doctor::doctor().await,
        Command::Version => {
            println!("clearmesh {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}
