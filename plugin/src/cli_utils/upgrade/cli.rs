use crate::cli_utils::upgrade::apply_upgrade;
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

/// Upgrade arguments.
#[derive(Debug, Parser)]
pub struct UpgradeArgs {
    #[arg(skip)]
    pub namespace: String,

    #[arg(skip)]
    pub kubeconfig: Option<PathBuf>,

    /// Helm release name for the openebs helm chart.
    #[arg(global = true, long, short)]
    pub release_name: Option<String>,

    /// Specify the container registry for the upgrade-job image.
    #[arg(global = true, long)]
    pub registry: Option<String>,

    /// Allow upgrade from stable versions to unstable versions. This is implied when the
    /// '--skip-upgrade-path-validation-for-unsupported-version' option is used.
    #[arg(global = true, long, hide = true, default_value_t = false)]
    pub allow_unstable: bool,

    /// Display all the validations output but will not execute upgrade.
    #[arg(global = true, long, short, default_value_t = false)]
    pub dry_run: bool,

    /// If set then upgrade will skip the io-engine pods restart.
    #[arg(global = true, long, default_value_t = false)]
    pub skip_data_plane_restart: bool,

    /// If set then it will continue with upgrade without validating singla replica volume.
    #[arg(global = true, long, default_value_t = false)]
    pub skip_single_replica_volume_validation: bool,

    /// If set then upgrade will skip the replica rebuild in progress validation.
    #[arg(global = true, long, default_value_t = false)]
    pub skip_replica_rebuild: bool,

    /// If set then upgrade will skip the cordoned node validation.
    #[arg(global = true, long, default_value_t = false)]
    pub skip_cordoned_node_validation: bool,

    /// Upgrade to an unsupported version.
    #[arg(global = true, hide = true, long, default_value_t = false)]
    pub skip_upgrade_path_validation_for_unsupported_version: bool,

    /// The set values on the command line.
    /// (can specify multiple or separate values with commas: key1=val1,key2=val2).
    #[arg(global = true, long)]
    pub set: Vec<String>,

    /// The set values from respective files specified via the command line
    /// (can specify multiple or separate values with commas: key1=path1,key2=path2).
    #[arg(global = true, long)]
    pub set_file: Vec<String>,

    /// This is the helm storage driver, e.g. secret, configmap, memory, etc.
    #[arg(env = "HELM_DRIVER", default_value = "")]
    pub helm_storage_driver: String,
}

impl UpgradeArgs {
    /// Start an upgrade based on supplied inputs.
    pub async fn apply(&self) -> Result<()> {
        apply_upgrade(self).await
    }
}
