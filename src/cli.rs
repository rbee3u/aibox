//! Command-line surface plus the argv pre-split that keeps pass-through agent
//! arguments away from clap.

use crate::agent::AgentKind;
use crate::component::ComponentSpec;
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};

/// Parsed aibox command line, excluding Coding Agent arguments after `--`.
#[derive(Debug, Parser)]
#[command(
    name = "aibox",
    about = "Run coding agents inside a Docker Filesystem Sandbox",
    subcommand_required = true,
    arg_required_else_help = true,
    version
)]
pub struct Cli {
    /// Operation to perform.
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Parse the process command line, printing clap errors before exiting.
    ///
    /// Production callers must split argv with [`split_passthrough`] first.
    pub fn parse() -> Self {
        Self::parse_from(std::env::args_os())
    }

    /// Parse a pre-split argument iterator, printing clap errors before exiting.
    pub fn parse_from<I, T>(itr: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        match Self::try_parse_from(itr) {
            Ok(cli) => cli,
            Err(error) => error.exit(),
        }
    }

    /// Parse arguments without exiting on an error.
    pub fn try_parse_from<I, T>(itr: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let args: Vec<OsString> = itr.into_iter().map(Into::into).collect();
        reject_duplicate_selection_options(&args)?;
        <Self as Parser>::try_parse_from(args)
    }
}

fn reject_duplicate_selection_options(args: &[OsString]) -> Result<(), clap::Error> {
    let command = args.get(1).and_then(|value| value.to_str());
    if !matches!(command, Some("run" | "component" | "provider" | "session")) {
        return Ok(());
    }
    let mut seen = BTreeSet::new();
    let mut index = 2;
    while index < args.len() {
        if args[index] == "--" {
            break;
        }
        let Some(token) = args[index].to_str() else {
            index += 1;
            continue;
        };
        let (name, takes_next) = match token {
            "--agent" => (Some("--agent"), true),
            "--tenant" => (Some("--tenant"), true),
            "--host" => (Some("--host"), false),
            value if value.starts_with("--agent=") => (Some("--agent"), false),
            value if value.starts_with("--tenant=") => (Some("--tenant"), false),
            _ => (None, false),
        };
        if let Some(name) = name {
            if !seen.insert(name) {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::ArgumentConflict,
                    format!("{name} must be provided only once in a command scope"),
                ));
            }
        }
        index += usize::from(takes_next) + 1;
    }
    Ok(())
}

/// Top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run a Coding Agent inside the aibox container.
    ///
    /// Pass arguments verbatim after `--`, for example:
    /// `aibox run -- "fix the build"`.
    Run(RunArgs),
    /// Build the aibox Docker image.
    Build(BuildArgs),
    /// Generate a shell completion registration script.
    Completion(CompletionArgs),
    /// Manage aibox-managed Tenants.
    Tenant(TenantArgs),
    /// Manage optional components in a Managed Tenant.
    Component(ComponentArgs),
    /// Manage Tenant-local Providers and Agent Configuration.
    Provider(ProviderArgs),
    /// Browse saved Sessions on the host without starting Docker.
    Session(SessionArgs),
}

/// Options for launching a Coding Agent in Docker.
#[derive(Debug, Args)]
pub struct RunArgs {
    /// Coding Agent to run. Omit for Codex.
    #[arg(id = "run-agent", long = "agent", value_name = "AGENT", value_enum)]
    pub agent: Option<AgentKind>,

    /// Managed Tenant name (default: `default`).
    #[arg(
        id = "run-tenant",
        long = "tenant",
        value_name = "TENANT",
        value_parser = parse_tenant
    )]
    pub tenant: Option<String>,

    /// Workspace mounted at /workspace (default: current directory).
    #[arg(short, long)]
    pub workspace: Option<String>,

    /// Extra bind mount, Docker syntax `host:container[:ro]` (repeatable).
    #[arg(short, long)]
    pub mount: Vec<String>,
}

impl RunArgs {
    /// Selected Managed Tenant, defaulting to `default`.
    pub fn tenant_name(&self) -> &str {
        self.tenant.as_deref().unwrap_or("default")
    }
}

/// Options for `aibox build`.
#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Disable Docker's build cache and pull a fresh Debian base image.
    #[arg(short, long)]
    pub force: bool,
}

/// Arguments for `aibox completion`.
#[derive(Debug, Args)]
pub struct CompletionArgs {
    /// Shell whose dynamic completion registration script to generate.
    #[arg(value_enum)]
    pub shell: CompletionShell,
}

/// Shells with supported aibox dynamic completion adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum CompletionShell {
    /// Bourne Again Shell.
    Bash,
    /// Z shell.
    Zsh,
    /// Friendly Interactive Shell.
    Fish,
}

/// Arguments for Managed Tenant management.
#[derive(Debug, Args)]
pub struct TenantArgs {
    /// Managed Tenant operation to perform.
    #[command(subcommand)]
    pub command: TenantCommand,
}

/// Managed Tenant management operations.
#[derive(Debug, Subcommand)]
pub enum TenantCommand {
    /// List Managed Tenants.
    List,
    /// Create or repair a Managed Tenant.
    Create {
        /// Managed Tenant to create.
        #[arg(value_parser = parse_tenant)]
        tenant: String,
    },
    /// Delete one or more Managed Tenants.
    Delete {
        /// Managed Tenant names to delete.
        #[arg(
            value_name = "TENANT",
            value_parser = parse_tenant,
            required_unless_present = "all",
            conflicts_with = "all"
        )]
        tenants: Vec<String>,
        /// Delete every Managed Tenant.
        #[arg(long)]
        all: bool,
        /// Skip delete confirmations.
        #[arg(short, long)]
        yes: bool,
    },
}

/// Managed Tenant Component arguments.
#[derive(Debug, Args)]
pub struct ComponentArgs {
    /// Managed Tenant whose Components to inspect or install (default: `default`).
    #[arg(
        id = "component-tenant",
        long = "tenant",
        value_name = "TENANT",
        value_parser = parse_tenant,
        global = true
    )]
    pub tenant: Option<String>,

    /// Component operation to perform.
    #[command(subcommand)]
    pub command: ComponentCommand,
}

impl ComponentArgs {
    /// Selected Managed Tenant, defaulting to `default`.
    pub fn tenant_name(&self) -> &str {
        self.tenant.as_deref().unwrap_or("default")
    }
}

/// Managed Tenant Component operations.
#[derive(Debug, Subcommand)]
pub enum ComponentCommand {
    /// List available Components and their state in the selected Tenant.
    List,
    /// Install or replace one Component in the selected Tenant.
    Install {
        /// Component name, optionally followed by a stable toolchain version.
        #[arg(value_name = "COMPONENT[@X.Y.Z]")]
        component: ComponentSpec,
    },
}

/// Mutually exclusive selection of a Managed Tenant or the Host Tenant.
#[derive(Debug, Args)]
pub struct TenantSelection {
    /// Managed Tenant name (default: `default`).
    #[arg(
        long = "tenant",
        value_name = "TENANT",
        value_parser = parse_tenant,
        global = true,
        conflicts_with = "host"
    )]
    pub tenant: Option<String>,

    /// Operate on the real host Coding Agent state.
    #[arg(long, global = true, conflicts_with = "tenant")]
    pub host: bool,
}

impl TenantSelection {
    /// Selected Managed Tenant name when the Host Tenant was not requested.
    pub fn tenant_name(&self) -> &str {
        self.tenant.as_deref().unwrap_or("default")
    }
}

/// Agent- and Tenant-scoped Provider management arguments.
#[derive(Debug, Args)]
pub struct ProviderArgs {
    /// Coding Agent whose Provider catalog and configuration to manage.
    #[arg(long = "agent", value_name = "AGENT", value_enum, global = true)]
    pub agent: Option<AgentKind>,

    /// Tenant whose Provider catalog and Agent Configuration to manage.
    #[command(flatten)]
    pub tenant: TenantSelection,

    /// Provider operation to perform.
    #[command(subcommand)]
    pub command: ProviderCommand,
}

/// Provider configuration operations.
#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
    /// List Providers in the selected Tenant and Coding Agent.
    List,
    /// Print one Provider file.
    Get {
        /// Provider to print.
        #[arg(value_parser = parse_provider)]
        provider: String,
        /// Print the credential file instead of the main configuration.
        #[arg(long)]
        auth: bool,
    },
    /// Create a Provider from the built-in connection template.
    Create {
        /// Provider to create.
        #[arg(value_parser = parse_provider)]
        provider: String,
    },
    /// Open a Provider file in `$VISUAL` or `$EDITOR`.
    Edit {
        /// Provider to edit.
        #[arg(value_parser = parse_provider)]
        provider: String,
        /// Edit the credential file instead of the main configuration.
        #[arg(long)]
        auth: bool,
    },
    /// Delete one or more inactive Providers.
    Delete {
        /// Provider names to delete.
        #[arg(
            value_name = "PROVIDER",
            value_parser = parse_provider,
            required_unless_present = "all",
            conflicts_with = "all"
        )]
        providers: Vec<String>,
        /// Delete every inactive Provider.
        #[arg(long)]
        all: bool,
        /// Skip delete confirmations.
        #[arg(short, long)]
        yes: bool,
    },
    /// Materialize a Provider into the selected Agent Configuration.
    Activate {
        /// Provider to activate.
        #[arg(value_parser = parse_provider)]
        provider: String,
        /// Irreversibly discard Agent Configuration changes since activation.
        #[arg(long)]
        discard_config_changes: bool,
    },
    /// Restore the pre-activation Agent Configuration and clear activation.
    Deactivate {
        /// Irreversibly discard Agent Configuration changes since activation.
        #[arg(long)]
        discard_config_changes: bool,
    },
    /// Classify divergence between applied, source, and working configuration.
    Status,
    /// Show applied-to-working and applied-to-source changes.
    Diff,
    /// Reconcile Provider source and working Agent Configuration.
    Reconcile(ReconcileArgs),
}

/// Conflict-resolution options for `provider reconcile`.
#[derive(Debug, Args)]
pub struct ReconcileArgs {
    /// Resolve a conflicting JSON Pointer with the Provider source value.
    #[arg(long = "take-provider", value_name = "JSON_POINTER")]
    pub take_provider: Vec<String>,
    /// Resolve a conflicting JSON Pointer with the Agent Configuration value.
    #[arg(long = "take-config", value_name = "JSON_POINTER")]
    pub take_config: Vec<String>,
    /// Resolve every conflict with Provider source values.
    #[arg(long, conflicts_with = "take_config_all")]
    pub take_provider_all: bool,
    /// Resolve every conflict with Agent Configuration values.
    #[arg(long, conflicts_with = "take_provider_all")]
    pub take_config_all: bool,
}

/// Agent- and Tenant-scoped Session browsing arguments.
#[derive(Debug, Args)]
pub struct SessionArgs {
    /// Coding Agent whose Sessions to browse. Omit for Codex.
    #[arg(long = "agent", value_name = "AGENT", value_enum, global = true)]
    pub agent: Option<AgentKind>,

    /// Tenant whose Sessions to browse.
    #[command(flatten)]
    pub tenant: TenantSelection,

    /// Session operation, or no operation for `list`.
    #[command(subcommand)]
    pub command: Option<SessionCommand>,
}

/// Saved Session operations.
#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// List Sessions, newest first.
    List,
    /// Print the prompts typed in one Session.
    Get {
        /// Full Session id or unique prefix.
        id: String,
    },
    /// Delete one or more Session transcripts.
    Delete {
        /// Full Session ids or unique prefixes.
        #[arg(
            value_name = "ID",
            required_unless_present = "all",
            conflicts_with = "all"
        )]
        ids: Vec<String>,
        /// Delete every Session transcript.
        #[arg(long)]
        all: bool,
        /// Skip delete confirmations.
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

fn parse_tenant(value: &str) -> Result<String, String> {
    crate::tenant::validate_name("tenant", value)
        .map(|()| value.to_string())
        .map_err(|error| error.to_string())
}

fn parse_provider(value: &str) -> Result<String, String> {
    crate::tenant::validate_name("provider", value)
        .map(|()| value.to_string())
        .map_err(|error| error.to_string())
}

/// Split argv at the first `--`. The boundary itself is dropped.
pub fn split_passthrough<T: AsRef<OsStr>>(argv: Vec<T>) -> (Vec<T>, Vec<T>) {
    match argv.iter().position(|arg| arg.as_ref() == OsStr::new("--")) {
        Some(index) => {
            let mut left = argv;
            let right = left.split_off(index + 1);
            left.pop();
            (left, right)
        }
        None => (argv, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn passthrough_uses_the_first_boundary() {
        let args = ["aibox", "run", "--tenant", "work", "--", "exec", "--"]
            .map(String::from)
            .to_vec();
        let (left, right) = split_passthrough(args);
        assert_eq!(left, ["aibox", "run", "--tenant", "work"]);
        assert_eq!(right, ["exec", "--"]);
    }

    #[test]
    fn host_tenant_is_distinct_from_managed_tenant_named_host() {
        let cli = Cli::try_parse_from(["aibox", "provider", "--tenant", "host", "list"]).unwrap();
        let Command::Provider(args) = cli.command else {
            panic!("expected provider command");
        };
        assert_eq!(args.tenant.tenant.as_deref(), Some("host"));
        assert!(!args.tenant.host);

        let cli = Cli::try_parse_from(["aibox", "provider", "--host", "list"]).unwrap();
        let Command::Provider(args) = cli.command else {
            panic!("expected provider command");
        };
        assert!(args.tenant.host);
        assert!(args.tenant.tenant.is_none());
    }

    #[test]
    fn tenant_selectors_conflict() {
        let error =
            Cli::try_parse_from(["aibox", "session", "--host", "--tenant", "default", "list"])
                .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn destructive_commands_require_explicit_selections() {
        for args in [
            vec!["aibox", "tenant", "delete"],
            vec!["aibox", "provider", "delete"],
            vec!["aibox", "session", "delete"],
        ] {
            let error = Cli::try_parse_from(args).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
        }
    }

    #[test]
    fn removed_provider_apply_is_rejected() {
        assert!(Cli::try_parse_from(["aibox", "provider", "apply", "custom"]).is_err());
    }

    #[test]
    fn run_rejects_host_selector() {
        assert!(Cli::try_parse_from(["aibox", "run", "--host"]).is_err());
        let cli = Cli::try_parse_from(["aibox", "run", "--tenant", "host"]).unwrap();
        let Command::Run(args) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(args.tenant_name(), "host");
    }

    #[test]
    fn component_scope_is_managed_tenant_only() {
        let cli = Cli::try_parse_from([
            "aibox",
            "component",
            "install",
            "rust@1.90.0",
            "--tenant",
            "work",
        ])
        .unwrap();
        let Command::Component(args) = cli.command else {
            panic!("expected component command");
        };
        assert_eq!(args.tenant_name(), "work");
        let ComponentCommand::Install { component } = args.command else {
            panic!("expected component install");
        };
        assert_eq!(component.to_string(), "rust@1.90.0");

        assert!(Cli::try_parse_from(["aibox", "component", "--host", "list"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "component", "--agent", "codex", "list"]).is_err());
    }
}
