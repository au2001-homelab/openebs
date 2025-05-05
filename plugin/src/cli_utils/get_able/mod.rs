use crate::cli_utils::upgrade::k8s::upgrade_status;
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

/// Args pertaining to the `kubectl openebs get upgrade-status` command specifically.
#[derive(Debug, Parser)]
pub struct UpgradeStatusArgs {
    /// Helm release name for the openebs helm chart.
    #[arg(global = true, long, short)]
    pub release_name: Option<String>,

    /// This is the helm storage driver, e.g. secret, configmap, memory, etc.
    #[arg(env = "HELM_DRIVER", default_value = "")]
    pub helm_storage_driver: String,
}

/// Command structure and common arguments related to `kubectl openebs get` commands.
#[derive(Debug, Parser)]
pub struct GetAble {
    /// Kubernetes namespace of GET-able resource.
    #[arg(skip)]
    pub namespace: String,

    /// Path to kubeconfig file.
    #[arg(skip)]
    pub kubeconfig: Option<PathBuf>,

    #[command(subcommand)]
    pub resources: Resource,
}

/// Resources which can be GET.
#[derive(Debug, Parser)]
pub enum Resource {
    /// Get details about an ongoing upgrade.
    UpgradeStatus(UpgradeStatusArgs),
}

impl GetAble {
    /// Perform a GET-like operation on a GetAble variant.
    pub async fn get(&self) -> Result<()> {
        match &self.resources {
            Resource::UpgradeStatus(args) => {
                upgrade_status::get_upgrade_status(
                    self.namespace.as_str(),
                    args.release_name.clone(),
                    args.helm_storage_driver.clone(),
                )
                .await
            }
        }
    }
}
