//! Database debugging tool

use crate::{
    args::{
        utils::{chain_help, genesis_value_parser, SUPPORTED_CHAINS},
        DatabaseArgs,
    },
    utils::DbTool,
};
use clap::{Parser, Subcommand};
use reth_db::{
    open_db, open_db_read_only,
    version::{get_db_version, DatabaseVersionError, DB_VERSION},
};
use reth_node_core::args::DatadirArgs;
use reth_primitives::ChainSpec;
use reth_provider::{providers::StaticFileProvider, ProviderFactory};
use std::{
    io::{self, Write},
    sync::Arc,
};

mod checksum;
mod clear;
mod diff;
mod get;
mod list;
mod stats;
/// DB List TUI
mod tui;

/// `reth db` command
#[derive(Debug, Parser)]
pub struct Command {
    /// The chain this node is running.
    ///
    /// Possible values are either a built-in chain or the path to a chain specification file.
    #[arg(
        long,
        value_name = "CHAIN_OR_PATH",
        long_help = chain_help(),
        default_value = SUPPORTED_CHAINS[0],
        value_parser = genesis_value_parser,
        global = true,
    )]
    chain: Arc<ChainSpec>,

    #[command(flatten)]
    datadir: DatadirArgs,

    #[command(flatten)]
    db: DatabaseArgs,

    #[command(subcommand)]
    command: Subcommands,
}

#[derive(Subcommand, Debug)]
/// `reth db` subcommands
pub enum Subcommands {
    /// Lists all the tables, their entry count and their size
    Stats(stats::Command),
    /// Lists the contents of a table
    List(list::Command),
    /// Calculates the content checksum of a table
    Checksum(checksum::Command),
    /// Create a diff between two database tables or two entire databases.
    Diff(diff::Command),
    /// Gets the content of a table for the given key
    Get(get::Command),
    /// Deletes all database entries
    Drop {
        /// Bypasses the interactive confirmation and drops the database directly
        #[arg(short, long)]
        force: bool,
    },
    /// Deletes all table entries
    Clear(clear::Command),
    /// Lists current and local database versions
    Version,
    /// Returns the full database path
    Path,
}

/// `db_ro_exec` opens a database in read-only mode, and then execute with the provided command
macro_rules! db_ro_exec {
    ($chain:expr, $db_path:expr, $db_args:ident, $sfp:ident, $tool:ident, $command:block) => {
        let db = open_db_read_only($db_path, $db_args)?;
        let provider_factory =
            ProviderFactory::new(db, $chain.clone(), StaticFileProvider::read_only($sfp)?);

        let $tool = DbTool::new(provider_factory)?;
        $command;
    };
}

impl Command {
    /// Execute `db` command
    pub async fn execute(self) -> eyre::Result<()> {
        // add network name to data dir
        let data_dir = self.datadir.resolve_datadir(self.chain.chain);
        let db_path = data_dir.db();
        let db_args = self.db.database_args();
        let static_files_path = data_dir.static_files();

        match self.command {
            // TODO: We'll need to add this on the DB trait.
            Subcommands::Stats(command) => {
                db_ro_exec!(self.chain, &db_path, db_args, static_files_path, tool, {
                    command.execute(data_dir, &tool)?;
                });
            }
            Subcommands::List(command) => {
                db_ro_exec!(self.chain, &db_path, db_args, static_files_path, tool, {
                    command.execute(&tool)?;
                });
            }
            Subcommands::Checksum(command) => {
                db_ro_exec!(self.chain, &db_path, db_args, static_files_path, tool, {
                    command.execute(&tool)?;
                });
            }
            Subcommands::Diff(command) => {
                db_ro_exec!(self.chain, &db_path, db_args, static_files_path, tool, {
                    command.execute(&tool)?;
                });
            }
            Subcommands::Get(command) => {
                db_ro_exec!(self.chain, &db_path, db_args, static_files_path, tool, {
                    command.execute(&tool)?;
                });
            }
            Subcommands::Drop { force } => {
                if !force {
                    // Ask for confirmation
                    print!("Are you sure you want to drop the database at {data_dir}? This cannot be undone. (y/N): ");
                    // Flush the buffer to ensure the message is printed immediately
                    io::stdout().flush().unwrap();

                    let mut input = String::new();
                    io::stdin().read_line(&mut input).expect("Failed to read line");

                    if !input.trim().eq_ignore_ascii_case("y") {
                        println!("Database drop aborted!");
                        return Ok(())
                    }
                }

                let db = open_db(&db_path, db_args)?;
                let provider_factory = ProviderFactory::new(
                    db,
                    self.chain.clone(),
                    StaticFileProvider::read_write(&static_files_path)?,
                );

                let tool = DbTool::new(provider_factory)?;
                tool.drop(db_path, static_files_path)?;
            }
            Subcommands::Clear(command) => {
                let db = open_db(&db_path, db_args)?;
                let provider_factory = ProviderFactory::new(
                    db,
                    self.chain.clone(),
                    StaticFileProvider::read_write(static_files_path)?,
                );

                command.execute(provider_factory)?;
            }
            Subcommands::Version => {
                let local_db_version = match get_db_version(&db_path) {
                    Ok(version) => Some(version),
                    Err(DatabaseVersionError::MissingFile) => None,
                    Err(err) => return Err(err.into()),
                };

                println!("Current database version: {DB_VERSION}");

                if let Some(version) = local_db_version {
                    println!("Local database version: {version}");
                } else {
                    println!("Local database is uninitialized");
                }
            }
            Subcommands::Path => {
                println!("{}", db_path.display());
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parse_stats_globals() {
        let path = format!("../{}", SUPPORTED_CHAINS[0]);
        let cmd = Command::try_parse_from(["reth", "--datadir", &path, "stats"]).unwrap();
        assert_eq!(cmd.datadir.resolve_datadir(cmd.chain.chain).as_ref(), Path::new(&path));
    }
}
