use crate::{
    cli_utils::upgrade::k8s::{
        delete_older_upgrade_events, helm_release_name, list_events,
        resources::{
            config_map_data, job_set_file_args, upgrade_configmap, upgrade_job_cluster_role,
            upgrade_job_cluster_role_binding, upgrade_job_service_account,
        },
        upgrade_name_concat,
    },
    console_logger,
    constants::{upgrade_obj_suffix, UPGRADE_JOB_IMAGE_REPO},
    upgrade_labels,
};
use anyhow::{anyhow, Result};
use cli::UpgradeArgs;
use k8s_openapi::{
    api::{
        batch::v1::{Job, JobSpec},
        core::v1::{
            ConfigMap, ConfigMapVolumeSource, Container, EnvVar, EnvVarSource, ExecAction,
            ObjectFieldSelector, PodSpec, PodTemplateSpec, Probe, ServiceAccount, Volume,
            VolumeMount,
        },
        events::v1::Event,
        rbac::v1::{ClusterRole, ClusterRoleBinding},
    },
    apimachinery::pkg::apis::meta::v1::ObjectMeta,
    kind,
};
use kube::{
    api::{Api, DeleteParams, PostParams},
    client::Client,
    Error as kubeError, ResourceExt,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashSet, env};
use tokio::{spawn, task::JoinHandle, try_join};
use upgrade::common::kube::client::list_pods;

pub mod cli;
pub mod k8s;

/// This type could be used to gather container image data from several sources and
/// then reasoning among these options and picking the most appropriate values.
pub struct ImageProperties {
    pub pull_secrets: Option<Vec<k8s_openapi::api::core::v1::LocalObjectReference>>,
    pub registry: String,
    pub pull_policy: Option<String>,
}

impl ImageProperties {
    /// Create an instance of ImageProperties from an openebs/openebs helm release.
    pub async fn new_from_helm_release(release_name: &str, args: &UpgradeArgs) -> Result<Self> {
        /* The strategy we use here assumes that users don't change the image name, i.e. the
         *     - image: <value>
         *       name: <value> // This one.
         * We find one of the CSI controllers from the CSI LocalPVs and Mayastor or the LocalPV
         * Provisioner.
         * Once we have found this Pod, we try to find on the containers that we know will exist
         * here. Other CSI sidecars may also exist, so we pick out our container based on container
         * name. As we know, we expect users to not change it, as it is of no use as far as a
         * container configuration is concerned. It identifies containers uniquely, and we already
         * achieve this with our names.
         * We split the 'image' of the containers we have found above based on the '/' character.
         * If the image could be split into 3 sections by splitting along '/', then we use the first
         * section as the image registry. This may not work for many case, and uses a naive
         * approach. For those cases, a user should use the `--registry <value>` option flag.
         * The same Pod's ImagePullSecrets and ImagePullPolicy sections are used to populate those
         * specific sections.
         */

        let hostpath_localpv_image_name = format!("{release_name}-localpv-provisioner");
        let openebs_containers: HashSet<&str> = [
            "api-rest",
            "openebs-zfs-plugin",
            "openebs-lvm-plugin",
            hostpath_localpv_image_name.as_str(),
        ]
        .into_iter()
        .collect();

        // The 'app in <labels>' is used to pick out the CSI Controllers from the LocalPV, Mayastor
        // and the Hostpath Provisioner.
        let openebs_pod_spec = list_pods(args.namespace.clone(), Some("app in (api-rest,localpv-provisioner,openebs-zfs-controller,openebs-lvm-controller)".to_string()), None).await
            .map_err(|error| anyhow!(error))?
            .into_iter()
            .filter_map(|pod| pod.spec)
            // Find the first Pod we encounter from among the above Pods.
            .find(|pod_spec| pod_spec.containers.iter().map(|container| container.name.as_str()).any(|name| openebs_containers.contains(&name)))
            .ok_or(anyhow!("Couldn't pick out an openebs container, one of '{openebs_containers:?}', from openebs Pods"))?;

        let openebs_container = openebs_pod_spec
            .containers
            .iter()
            .find(|c| openebs_containers.contains(c.name.as_str()))
            .unwrap();
        Ok(Self {
            pull_secrets: openebs_pod_spec.image_pull_secrets,
            registry: args
                .registry
                .clone()
                .or(openebs_container.image.as_deref().and_then(|img| {
                    let parts: Vec<&str> = img.split('/').collect();
                    (parts.len() == 3).then(|| parts[0].to_owned())
                }))
                .unwrap_or("docker.io".to_owned()),
            pull_policy: openebs_container.image_pull_policy.clone(),
        })
    }
}

/// Returns a fully prepared upgrade-job object.
async fn upgrade_job(args: &UpgradeArgs, release_name: &str, set_file: String) -> Result<Job> {
    let image_properties: ImageProperties =
        ImageProperties::new_from_helm_release(release_name, args).await?;
    let upgrade_image = format!(
        "{image_registry}/{UPGRADE_JOB_IMAGE_REPO}/openebs-upgrade-job:{image_tag}",
        image_registry = image_properties.registry,
        image_tag = upgrade_obj_suffix()
    );

    let helm_args_set = args.set.join(",");
    let mut job_args: Vec<String> = vec![
        format!("--rest-endpoint=http://{release_name}-api-rest:8081"),
        format!(
            "--namespace={namespace}",
            namespace = args.namespace.as_str()
        ),
        format!("--release-name={release_name}"),
        format!("--helm-args-set={helm_args_set}"),
        format!("--helm-args-set-file={set_file}"),
    ];
    if args.skip_data_plane_restart {
        job_args.push("--skip-data-plane-restart".to_string());
    }
    if args.skip_upgrade_path_validation_for_unsupported_version {
        job_args.push("--skip-upgrade-path-validation".to_string());
    }

    Ok(Job {
        metadata: ObjectMeta {
            labels: Some(upgrade_labels!()),
            name: Some(format!(
                "{release_name}-upgrade-{version}",
                version = upgrade_obj_suffix()
            )),
            namespace: Some(args.namespace.clone()),
            ..Default::default()
        },
        spec: Some(JobSpec {
            // Backoff for unrecoverable errors, recoverable errors are handled by the Job process
            // Investigate backoff with `kubectl -n <namespace> logs job/<job-name>`.
            // Non-recoverable errors also often emit Job event, `kubectl openebs get
            // upgrade-status` fetches the most recent Job event.
            backoff_limit: Some(6),
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(upgrade_labels!()),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    image_pull_secrets: image_properties.pull_secrets,
                    restart_policy: Some("OnFailure".to_string()),
                    containers: vec![Container {
                        args: Some(job_args),
                        image: Some(upgrade_image),
                        image_pull_policy: image_properties.pull_policy,
                        name: "openebs-upgrade-job".to_string(),
                        env: Some(vec![
                            EnvVar {
                                name: "RUST_LOG".to_string(),
                                value: Some(env::var("RUST_LOG").unwrap_or("info".to_string())),
                                ..Default::default()
                            },
                            EnvVar {
                                name: "POD_NAME".to_string(),
                                value_from: Some(EnvVarSource {
                                    field_ref: Some(ObjectFieldSelector {
                                        field_path: "metadata.name".to_string(),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            },
                            EnvVar {
                                // Ref: https://github.com/helm/helm/blob/main/cmd/helm/helm.go#L76
                                name: "HELM_DRIVER".to_string(),
                                value: env::var("HELM_DRIVER").ok(),
                                ..Default::default()
                            },
                        ]),
                        liveness_probe: Some(Probe {
                            exec: Some(ExecAction {
                                command: Some(vec!["pgrep".to_string(), "upgrade-job".to_string()]),
                            }),
                            initial_delay_seconds: Some(10),
                            period_seconds: Some(60),
                            ..Default::default()
                        }),
                        volume_mounts: Some(vec![VolumeMount {
                            read_only: Some(true),
                            mount_path: "/upgrade-config-map".to_string(),
                            name: "upgrade-config-map".to_string(),
                            ..Default::default()
                        }]),
                        ..Default::default()
                    }],
                    service_account_name: Some(format!(
                        "{release_name}-upgrade-service-account-{version}",
                        version = upgrade_obj_suffix()
                    )),
                    volumes: Some(vec![Volume {
                        name: "upgrade-config-map".to_string(),
                        config_map: Some(ConfigMapVolumeSource {
                            name: Some(format!(
                                "{release_name}-upgrade-config-map-{version}",
                                version = upgrade_obj_suffix()
                            )),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Returns the first instance of upgrade-job Event that we find for this version of upgrade.
pub async fn get_latest_upgrade_event(
    namespace: &str,
    upgrade_events_field_selector: &str,
) -> Result<Event> {
    list_events(
        namespace,
        None,
        Some(upgrade_events_field_selector.to_string()),
    )
    .await?
    .into_iter()
    .find(|e| e.reason == Some("OpenebsUpgrade".to_string()))
    .ok_or(anyhow!("No upgrade event present"))
}

/// This is used to deserialize the JSON data present in an upgrade-job event.
#[derive(Clone, Deserialize)]
#[serde(rename_all(deserialize = "camelCase"))]
pub(crate) struct UpgradeEvent {
    from_version: String,
    to_version: String,
    message: String,
}

/// The initial upgrade-job Event is logged appropriately, else a failure is logged.
async fn handle_upgrade_event(
    latest_event: Event,
    release_name: &str,
    namespace: &str,
    k8s_client: Client,
) -> Result<()> {
    if let Some(action) = latest_event.action {
        if action == "Validation Failed" {
            if let Some(data) = latest_event.note {
                let ev: UpgradeEvent =
                    serde_json::from_str(data.as_str()).map_err(|err| anyhow!(err))?;
                console_logger::error("The validation for upgrade has failed, hence deleting the upgrade resources. Please re-run upgrade with valid values", ev.message.as_str());

                delete_upgrade_resources(release_name, namespace, k8s_client).await?;
            } else {
                return Err(anyhow!("Note not present in upgrade event"));
            }
        } else {
            console_logger::info("The upgrade has started\nYou can see the recent upgrade status using `get upgrade-status` command", None);
        }
    }

    Ok(())
}

/// Start upgrade by creating an upgrade-job and such.
pub async fn apply_upgrade(args: &UpgradeArgs) -> Result<()> {
    let k8s_client = kube_proxy::client_from_kubeconfig(args.kubeconfig.clone()).await?;
    let release_name = match args.release_name.as_ref() {
        Some(name) => name.clone(),
        None => {
            helm_release_name(args.namespace.as_str(), args.helm_storage_driver.as_str()).await?
        }
    };

    let upgrade_events_field_selector = format!(
        "involvedObject.kind=Job,involvedObject.name={name}",
        name = upgrade_name_concat(release_name.as_str(), "upgrade")
    );

    delete_older_upgrade_events(
        Api::namespaced(k8s_client.clone(), args.namespace.as_str()),
        upgrade_events_field_selector.as_str(),
    )
    .await?;

    create_upgrade_resources(args, release_name.as_str(), k8s_client.clone()).await?;

    for _ in 0..6 {
        // wait for 10 seconds for the upgrade event to be published
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        match get_latest_upgrade_event(
            args.namespace.as_str(),
            upgrade_events_field_selector.as_str(),
        )
        .await
        {
            Ok(latest_event) => {
                handle_upgrade_event(
                    latest_event,
                    release_name.as_str(),
                    args.namespace.as_str(),
                    k8s_client,
                )
                .await?
            }
            Err(_) => continue,
        }
        break;
    }

    Ok(())
}

/// Flatten Tokio errors from spawning tasks and errors from failed (yet successfully spawn-ed)
/// tasks.
async fn joined_flatten<T>(handle: JoinHandle<Result<T>>) -> Result<T> {
    match handle.await.map_err(|err| anyhow!(err)) {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(err),
        Err(err) => Err(err),
    }
}

/// Create upgrade kubernetes resources.
pub async fn create_upgrade_resources(
    args: &UpgradeArgs,
    release_name: &str,
    k8s_client: Client,
) -> Result<()> {
    let creation_log = |kind: &str, name: String, namespace: Option<String>| -> String {
        if let Some(namespace) = namespace {
            return format!("Created {kind} '{name}' in the '{namespace}' namespace");
        }
        format!("Created {kind} '{name}'")
    };

    let sa = upgrade_job_service_account(
        Some(args.namespace.clone()),
        upgrade_name_concat(release_name, "upgrade-service-account"),
    );
    let sa_client: Api<ServiceAccount> =
        Api::namespaced(k8s_client.clone(), args.namespace.as_str());

    let cluster_role_binding =
        upgrade_job_cluster_role_binding(Some(args.namespace.clone()), release_name.to_string());
    let cluster_role_binding_client: Api<ClusterRoleBinding> = Api::all(k8s_client.clone());

    let cluster_role = upgrade_job_cluster_role(
        Some(args.namespace.clone()),
        upgrade_name_concat(release_name, "upgrade-cluster-role"),
    );
    let cluster_role_client: Api<ClusterRole> = Api::all(k8s_client.clone());

    let cm_data = config_map_data(args.set_file.as_slice())?;
    let cm = upgrade_configmap(cm_data.0, args.namespace.as_str(), release_name.to_string());
    let cm_client: Api<ConfigMap> = Api::namespaced(k8s_client.clone(), args.namespace.as_str());

    let set_file = job_set_file_args(args.set_file.as_slice(), Some(cm_data.1))?;
    let job = upgrade_job(args, release_name, set_file.unwrap_or_default()).await?;
    let job_client: Api<Job> = Api::namespaced(k8s_client.clone(), args.namespace.as_str());

    try_join!(
        joined_flatten(spawn(idempotent_create_resource(
            sa_client,
            sa.clone(),
            Some(creation_log(
                "ServiceAccount",
                sa.name_unchecked(),
                sa.namespace()
            ))
        ))),
        joined_flatten(spawn(idempotent_create_resource(
            cluster_role_binding_client,
            cluster_role_binding.clone(),
            Some(creation_log(
                "ClusterRoleBinding",
                cluster_role_binding.name_unchecked(),
                None
            ))
        ))),
        joined_flatten(spawn(idempotent_create_resource(
            cluster_role_client,
            cluster_role.clone(),
            Some(creation_log(
                "ClusterRole",
                cluster_role.name_unchecked(),
                None
            ))
        ))),
        joined_flatten(spawn(idempotent_create_resource(
            cm_client,
            cm.clone(),
            Some(creation_log(
                "ConfigMap",
                cm.name_unchecked(),
                cm.namespace()
            ))
        ))),
        joined_flatten(spawn(idempotent_create_resource(
            job_client,
            job.clone(),
            Some(creation_log("Job", job.name_unchecked(), job.namespace()))
        ))),
    )?;

    Ok(())
}

/// Delete upgrade kubernetes resources.
pub async fn delete_upgrade_resources(
    release_name: &str,
    ns: &str,
    k8s_client: Client,
) -> Result<()> {
    let deletion_log = |kind: &str, name: String, namespace: Option<String>| -> String {
        if let Some(namespace) = namespace {
            return format!("Deleted {kind} '{name}' from the '{namespace}' namespace");
        }
        format!("Deleted {kind} '{name}'")
    };
    let version = upgrade_obj_suffix();

    let job_client: Api<Job> = Api::namespaced(k8s_client.clone(), ns);
    let job_name = format!(
        "{release_name}-upgrade-{version}",
        version = version.as_str()
    );

    let cm_client: Api<ConfigMap> = Api::namespaced(k8s_client.clone(), ns);
    let cm_name = format!(
        "{release_name}-upgrade-config-map-{version}",
        version = version.as_str()
    );

    let cluster_role_client: Api<ClusterRole> = Api::all(k8s_client.clone());
    let cluster_role_name = upgrade_name_concat(release_name, "upgrade-cluster-role");

    let cluster_role_binding_client: Api<ClusterRoleBinding> = Api::all(k8s_client.clone());
    let cluster_role_binding_name = format!(
        "{release_name}-upgrade-role-binding-{version}",
        version = version.as_str()
    );

    let sa_client: Api<ServiceAccount> = Api::namespaced(k8s_client.clone(), ns);
    let sa_name = upgrade_name_concat(release_name, "upgrade-service-account");

    try_join!(
        joined_flatten(spawn(idempotent_delete_resource(
            job_client,
            job_name.clone(),
            Some(deletion_log("Job", job_name.clone(), Some(ns.to_string())))
        ))),
        joined_flatten(spawn(idempotent_delete_resource(
            cm_client,
            cm_name.clone(),
            Some(deletion_log(
                "ConfigMap",
                cm_name.clone(),
                Some(ns.to_string())
            ))
        ))),
        joined_flatten(spawn(idempotent_delete_resource(
            cluster_role_client,
            cluster_role_name.clone(),
            Some(deletion_log("ClusterRole", cluster_role_name.clone(), None))
        ))),
        joined_flatten(spawn(idempotent_delete_resource(
            cluster_role_binding_client,
            cluster_role_binding_name.clone(),
            Some(deletion_log(
                "ClusterRoleBinding",
                cluster_role_binding_name.clone(),
                None
            ))
        ))),
        joined_flatten(spawn(idempotent_delete_resource(
            sa_client,
            sa_name.clone(),
            Some(deletion_log(
                "ServiceAccount",
                sa_name.clone(),
                Some(ns.to_string())
            ))
        ))),
    )?;

    Ok(())
}

/// Create a kubernetes resource if an object of the same doesn't already exist.
pub async fn idempotent_create_resource<K>(
    client: Api<K>,
    resource: K,
    log: Option<String>,
) -> Result<()>
where
    K: k8s_openapi::Resource
        + Clone
        + std::fmt::Debug
        + kube::Resource
        + DeserializeOwned
        + Serialize,
{
    let pp = PostParams::default();
    client
        .create(&pp, &resource)
        .await
        .map(|_| {
            if let Some(log_line) = log {
                println!("{log_line}");
            }
        })
        .or_else(|err| match err {
            kubeError::Api(response) if response.reason.eq("AlreadyExists") => {
                println!(
                    "{kind} '{name}' already exists",
                    kind = kind(&resource),
                    name = resource.name_unchecked()
                );
                Ok(())
            }
            other => Err(anyhow!(other)),
        })
}

/// Delete a kubernetes object, if an object wih the same name exists.
pub async fn idempotent_delete_resource<K>(
    client: Api<K>,
    resource_name: String,
    log: Option<String>,
) -> Result<()>
where
    K: k8s_openapi::Resource + Clone + std::fmt::Debug + kube::Resource + DeserializeOwned,
{
    let dp = DeleteParams::foreground();
    client
        .delete(resource_name.as_str(), &dp)
        .await
        .map(|_| {
            if let Some(log_line) = log {
                println!("{log_line}");
            }
        })
        .or_else(|err| match err {
            kubeError::Api(response) if response.reason.eq("NotFound") => Ok(()),
            other => Err(anyhow!(other)),
        })
}
