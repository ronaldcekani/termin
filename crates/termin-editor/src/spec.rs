//! Command completion spec definitions (owned and static).
//!
//! See `docs/specs/input-editor.md §4.2`.

use serde::{Deserialize, Serialize};

/// Owned Command completion specification (parsed from TOML).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSpec {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub subcommands: Vec<SubcommandSpec>,
    #[serde(default)]
    pub options: Vec<OptionSpec>,
    #[serde(default)]
    pub args: Vec<ArgSpec>,
    #[serde(default)]
    pub persistent_options: Vec<OptionSpec>,
}

/// Owned Subcommand completion specification.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubcommandSpec {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub subcommands: Vec<SubcommandSpec>,
    #[serde(default)]
    pub options: Vec<OptionSpec>,
    #[serde(default)]
    pub args: Vec<ArgSpec>,
}

/// Owned Option/Flag specification.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionSpec {
    pub names: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub args: Vec<ArgSpec>,
    #[serde(default)]
    pub repeatable: bool,
    #[serde(default)]
    pub exclusive_with: Vec<String>,
}

/// Owned Positional Argument specification.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgSpec {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub variadic: bool,
    #[serde(default)]
    pub suggestions: Vec<String>,
    #[serde(default)]
    pub generators: Vec<String>,
    #[serde(default)]
    pub template: Option<String>,
}

// ============================================================================
// Static Spec Tables (Compiled, Zero-Allocation)
// ============================================================================

/// Static Command specification compiled into binary rodata.
#[derive(Copy, Clone, Debug)]
pub struct StaticCommandSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub subcommands: &'static [StaticSubcommandSpec],
    pub options: &'static [StaticOptionSpec],
    pub args: &'static [StaticArgSpec],
    pub persistent_options: &'static [StaticOptionSpec],
}

impl StaticCommandSpec {
    pub fn find_subcommand(&self, name_or_alias: &str) -> Option<&'static StaticSubcommandSpec> {
        self.subcommands
            .iter()
            .find(|sub| sub.name == name_or_alias || sub.aliases.contains(&name_or_alias))
    }

    pub fn find_option(&self, flag_name: &str) -> Option<&'static StaticOptionSpec> {
        self.options
            .iter()
            .chain(self.persistent_options.iter())
            .find(|opt| opt.names.contains(&flag_name))
    }
}

/// Static Subcommand specification compiled into binary rodata.
#[derive(Copy, Clone, Debug)]
pub struct StaticSubcommandSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub subcommands: &'static [StaticSubcommandSpec],
    pub options: &'static [StaticOptionSpec],
    pub args: &'static [StaticArgSpec],
}

impl StaticSubcommandSpec {
    pub fn find_subcommand(&self, name_or_alias: &str) -> Option<&'static StaticSubcommandSpec> {
        self.subcommands
            .iter()
            .find(|sub| sub.name == name_or_alias || sub.aliases.contains(&name_or_alias))
    }

    pub fn find_option(&self, flag_name: &str) -> Option<&'static StaticOptionSpec> {
        self.options
            .iter()
            .find(|opt| opt.names.contains(&flag_name))
    }
}

/// Static Option specification compiled into binary rodata.
#[derive(Copy, Clone, Debug)]
pub struct StaticOptionSpec {
    pub names: &'static [&'static str],
    pub description: &'static str,
    pub args: &'static [StaticArgSpec],
    pub repeatable: bool,
}

/// Static Argument specification compiled into binary rodata.
#[derive(Copy, Clone, Debug)]
pub struct StaticArgSpec {
    pub name: &'static str,
    pub optional: bool,
    pub variadic: bool,
    pub suggestions: &'static [&'static str],
    pub generators: &'static [&'static str],
    pub template: Option<&'static str>,
}
