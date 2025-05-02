pub use constants::UPGRADE_JOB_IMAGE_REPO;
pub use openebs_upgrade::constants::HTTP_DATA_PAGE_SIZE;
use utils::version_info;

/// The name suffix to the kubernetes upgrade-job resources and related resources.
pub fn upgrade_obj_suffix() -> String {
    version_info!()
        .version_tag
        .unwrap_or(UPGRADE_JOB_IMAGE_TAG.to_string())
        .replace('.', "-")
}

/// The default image tag to the upgrade-job image.
pub const UPGRADE_JOB_IMAGE_TAG: &str = "develop";
/// The default image registry for container images.
pub const DEFAULT_IMAGE_REGISTRY: &str = "docker.io";
/// ConfigMap mount path for upgrade.
pub const UPGRADE_CONFIG_MAP_MOUNT_PATH: &str = "/upgrade-config-map";

pub const PRODUCT_NAME: &str = "openebs";
