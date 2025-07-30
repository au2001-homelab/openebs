#!/usr/bin/env bash
# Manage kubectl plugin binaries as OCI artifacts.
# Supports both pushing and pulling operations.
# Usage:
#   Push: ./kubectl-oci.sh push --tag <tag> --namespace <namespace> --username <user> --password <token>
#   Pull: ./kubectl-oci.sh pull --tag <tag> --namespace <namespace> --username <user> --password <token>

set -euo pipefail

SCRIPT_DIR="$(dirname "$(realpath "${BASH_SOURCE[0]:-"$0"}")")"
ROOT_DIR="$SCRIPT_DIR/../.."

source "$ROOT_DIR/mayastor/scripts/utils/log.sh"

ACTION=""
TAG=""
NAMESPACE=""
USERNAME=""
PASSWORD=""
REGISTRY="ghcr.io"
PLUGIN="${PLUGIN:-"openebs"}"

usage() {
  cat << EOF
Usage: $0 <action> [options]

Actions:
  push    Push kubectl binaries to $REGISTRY as OCI artifacts
  pull    Pull kubectl binaries from $REGISTRY OCI artifacts

Options:
  --tag <tag>             Release tag (required)
  --registry <registry>   The registry to push/pull from [default=$REGISTRY]
  --namespace <namespace> Namespace path (required)
  --username <username>   Registry username (required)
  --password <password>   Registry token/password (required)

Examples:
  $0 push --tag v1.0.0 --namespace $PLUGIN/dev --username user --password token
  $0 pull --tag v1.0.0 --namespace $PLUGIN/dev --username user --password token
EOF
}

parse_args() {
  if [[ $# -lt 1 ]]; then
    usage
    log_fatal "Error: Action required (push or pull)"
  fi

  ACTION="$1"
  shift

  case "$ACTION" in
    push|pull)
      ;;
    *)
      usage
      log_fatal "Error: Invalid action '$ACTION'. Must be 'push' or 'pull'"
      ;;
  esac

  while [[ $# -gt 0 ]]; do
    case $1 in
      --tag)
        TAG="$2"
        shift 2
        ;;
      --namespace)
        NAMESPACE="$2"
        shift 2
        ;;
      --registry)
        REGISTRY="$2"
        shift 2
        ;;
      --username)
        USERNAME="$2"
        shift 2
        ;;
      --password)
        PASSWORD="$2"
        shift 2
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        usage
        log_fatal "Unknown option: $1"
        ;;
    esac
  done

  if [[ -z "$TAG" ]] || [[ -z "$NAMESPACE" ]] || [[ -z "$USERNAME" ]] || [[ -z "$PASSWORD" ]]; then
    usage
    log_fatal "Error: All options (--tag, --namespace, --username, --password) are required"
  fi

  REPOSITORY=${REGISTRY}/${NAMESPACE}/kubectl-${PLUGIN}
}

# Login to registry
login_registry() {
  echo "Logging in to ${REGISTRY}..."
  echo "${PASSWORD}" | oras login "${REGISTRY}" --username "${USERNAME}" --password-stdin
}

# Push kubectl binaries to registry
push_artifacts() {
  echo "Pushing kubectl binaries to ${REPOSITORY} with tag ${TAG}"

  # Check if artifacts directory exists
  if [[ ! -d "artifacts" ]]; then
    log_fatal "Error: artifacts directory not found"
  fi

  # Create a combined tarball of all kubectl binaries
  echo "Creating combined tarball of all kubectl binaries..."
  local combined_tar="kubectl-$PLUGIN-all-platforms-${TAG}.tar.gz"

  tar -czf "${combined_tar}" -C artifacts .

  echo "Pushing combined tarball to ${REPOSITORY}:${TAG}"

  oras push "${REPOSITORY}:${TAG}" \
    --artifact-type application/vnd.$PLUGIN.kubectl.bundle.v1+tar+gzip \
    "${combined_tar}"

  rm -f "${combined_tar}"

  echo "✓ All kubectl binaries pushed successfully as a single bundle!"
  echo "Bundle available at: ${REPOSITORY}:${TAG}"
}

# Pull kubectl binaries from registry
pull_artifacts() {
  echo "Pulling kubectl binaries bundle from ${REPOSITORY} for release ${TAG}"

  echo "Pulling kubectl bundle..."
  oras pull "${REPOSITORY}:${TAG}"

  local bundle_tar="kubectl-$PLUGIN-all-platforms-${TAG}.tar.gz"

  if [[ ! -f "$bundle_tar" ]]; then
    log_fatal "Error: Could not find kubectl bundle tarball"
  fi

  echo "Extracting bundle to artifacts directory"

  mkdir -p artifacts/
  tar -xzf "$bundle_tar" -C artifacts/
  rm -f "$bundle_tar"

  echo "Contents of artifacts directory after extraction:"
  ls -la artifacts/

  echo "✓ All kubectl binaries pulled successfully!"
}

main() {
  parse_args "$@"
  login_registry

  case "$ACTION" in
    push)
      push_artifacts
      ;;
    pull)
      pull_artifacts
      ;;
  esac
}

main "$@"
