//! Command-line argument definitions (`clap` derive).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use shared_types::Profile;

/// Top-level CLI arguments for `opti-cli`.
#[derive(Debug, Parser)]
#[command(
    name = "opti-cli",
    version,
    about = "Transparent, hardware-aware Windows 11 optimisation CLI"
)]
pub struct Cli {
    /// Path to the SQLite state database.
    /// Defaults to %ProgramData%\WinOptSuite\state.db if ProgramData is set, else ./state.db.
    #[arg(long, global = true, value_name = "PATH")]
    pub db: Option<PathBuf>,

    /// Directory containing tweak manifest *.toml files.
    /// Defaults to manifests/tweaks next to the executable if present, else ./manifests/tweaks.
    #[arg(long, global = true, value_name = "DIR")]
    pub manifests: Option<PathBuf>,

    /// Emit machine-readable JSON output.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

/// Supported CLI subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Validate all tweak manifests and report counts by category and profile.
    Validate,
    /// Probe hardware, OS build, environment tags, and anti-cheat drivers.
    Scan,
    /// Compute a read-only old -> new diff and conflict warnings without modifying the system.
    DryRun(SelectionArgs),
    /// Apply a profile or explicit tweaks as an atomic rollbackable transaction.
    Apply(ApplyArgs),
    /// Roll back a committed transaction in reverse step order.
    Rollback {
        /// Transaction ID to roll back.
        #[arg(value_name = "TX_ID")]
        tx_id: String,
    },
    /// List incomplete transactions or recover a specific transaction.
    Recover {
        /// Optional transaction ID to recover; omit to list incomplete transactions.
        #[arg(value_name = "TX_ID")]
        tx_id: Option<String>,
    },
    /// Show transaction history (newest first).
    History,
    /// Compare live system state against the newest committed baseline.
    Drift,
    /// Run diagnostic checks on platform, database, catalog, providers, and probe.
    Doctor,
}

/// Curated profile choice mapped to [`shared_types::Profile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CliProfile {
    Safe,
    Privacy,
    Gaming,
    Laptop,
}

impl From<CliProfile> for Profile {
    fn from(value: CliProfile) -> Self {
        match value {
            CliProfile::Safe => Profile::Safe,
            CliProfile::Privacy => Profile::Privacy,
            CliProfile::Gaming => Profile::Gaming,
            CliProfile::Laptop => Profile::Laptop,
        }
    }
}

/// Mutually exclusive tweak selection: either `--profile` or one or more `--tweak` IDs.
#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct SelectionArgs {
    /// Select all tweaks belonging to a curated profile.
    #[arg(long, value_enum)]
    pub profile: Option<CliProfile>,

    /// Select one or more explicit tweak IDs.
    #[arg(long = "tweak", value_name = "ID", num_args = 1..)]
    pub tweaks: Vec<String>,
}

/// Arguments for the `apply` subcommand.
#[derive(Debug, Args)]
pub struct ApplyArgs {
    #[command(flatten)]
    pub selection: SelectionArgs,

    /// Skip the interactive y/N confirmation prompt.
    #[arg(long)]
    pub yes: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_global_flags_before_and_after_subcommand() {
        let cli = Cli::try_parse_from([
            "opti-cli",
            "--db",
            "/tmp/state.db",
            "--manifests",
            "/tmp/manifests",
            "--json",
            "validate",
        ])
        .unwrap();
        assert_eq!(cli.db, Some(PathBuf::from("/tmp/state.db")));
        assert_eq!(cli.manifests, Some(PathBuf::from("/tmp/manifests")));
        assert!(cli.json);
        assert!(matches!(cli.command, Command::Validate));

        let after = Cli::try_parse_from(["opti-cli", "history", "--json", "--db", "/tmp/other.db"])
            .unwrap();
        assert_eq!(after.db, Some(PathBuf::from("/tmp/other.db")));
        assert!(after.json);
        assert!(matches!(after.command, Command::History));
    }

    #[test]
    fn parses_dry_run_and_apply_selection_variants() {
        let dry_profile =
            Cli::try_parse_from(["opti-cli", "dry-run", "--profile", "gaming"]).unwrap();
        match dry_profile.command {
            Command::DryRun(sel) => {
                assert_eq!(sel.profile, Some(CliProfile::Gaming));
                assert!(sel.tweaks.is_empty());
            }
            other => panic!("unexpected {other:?}"),
        }

        let dry_tweaks = Cli::try_parse_from([
            "opti-cli",
            "dry-run",
            "--tweak",
            "privacy.disable-telemetry",
            "--tweak",
            "gaming.hags",
        ])
        .unwrap();
        match dry_tweaks.command {
            Command::DryRun(sel) => {
                assert_eq!(sel.profile, None);
                assert_eq!(sel.tweaks, vec!["privacy.disable-telemetry", "gaming.hags"]);
            }
            other => panic!("unexpected {other:?}"),
        }

        let apply_yes =
            Cli::try_parse_from(["opti-cli", "apply", "--tweak", "a.b", "c.d", "--yes"]).unwrap();
        match apply_yes.command {
            Command::Apply(args) => {
                assert!(args.yes);
                assert_eq!(args.selection.tweaks, vec!["a.b", "c.d"]);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_or_conflicting_selection() {
        assert!(Cli::try_parse_from(["opti-cli", "dry-run"]).is_err());
        assert!(
            Cli::try_parse_from(["opti-cli", "dry-run", "--profile", "safe", "--tweak", "a.b"])
                .is_err()
        );
        assert!(Cli::try_parse_from(["opti-cli", "apply", "--yes"]).is_err());
        assert!(Cli::try_parse_from(["opti-cli", "dry-run", "--profile", "unknown"]).is_err());
    }

    #[test]
    fn parses_remaining_subcommands() {
        assert!(matches!(
            Cli::try_parse_from(["opti-cli", "scan"]).unwrap().command,
            Command::Scan
        ));
        match Cli::try_parse_from(["opti-cli", "rollback", "tx-123"])
            .unwrap()
            .command
        {
            Command::Rollback { tx_id } => assert_eq!(tx_id, "tx-123"),
            other => panic!("unexpected {other:?}"),
        }
        match Cli::try_parse_from(["opti-cli", "recover"])
            .unwrap()
            .command
        {
            Command::Recover { tx_id } => assert_eq!(tx_id, None),
            other => panic!("unexpected {other:?}"),
        }
        match Cli::try_parse_from(["opti-cli", "recover", "tx-999"])
            .unwrap()
            .command
        {
            Command::Recover { tx_id } => assert_eq!(tx_id.as_deref(), Some("tx-999")),
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(
            Cli::try_parse_from(["opti-cli", "drift"]).unwrap().command,
            Command::Drift
        ));
        assert!(matches!(
            Cli::try_parse_from(["opti-cli", "doctor"]).unwrap().command,
            Command::Doctor
        ));
    }
}
