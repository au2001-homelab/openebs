---
oep-number: OEP 3916
title: Volume Cloning Feature for OpenEBS Mayastor
authors:
  - "@adam-lithus"
owners:
  - "@tiagolobocastro"
  - "@adam-lithus"
editor: TBD
creation-date: 2025-04-30
last-updated: 2025-04-30
status: provisional
---

# Volume Cloning Feature for OpenEBS Mayastor

## Table of Contents

1. [Overview](#overview)
2. [Motivation](#motivation)
3. [Goals](#goals)
4. [Non-Goals](#non-goals)
5. [Proposal](#proposal)
6. [User Stories](#user-stories)
7. [Implementation Details](#implementation-details)
8. [Testing](#testing)

---

## Overview

This proposal introduces a volume cloning feature for Mayastor. A clone is a copy of
an existing volume that can be used independently from the source volume. The feature
will leverage Mayastor's existing snapshot and restore capabilities to efficiently
implement cloning with copy-on-write (COW) functionality.

Volume cloning will enable users to quickly create duplicates of volumes for various
use cases such as development, testing, data analysis, and backup, without having to
copy all the data.

## Motivation

This feature addresses the need identified in
[GitHub issue #1718](https://github.com/openebs/mayastor/issues/1718).

Volume cloning addresses several key use cases:

1. **Development and Testing**: Developers can create clones of production volumes to
   use in test environments without affecting production data.
2. **Rapid Deployment**: Users can create templates or golden images and clone them
   for quick deployment of new instances.
3. **Data Analysis**: Data scientists can clone datasets to perform analysis without
   risking changes to the original data.
4. **Backup and Recovery**: Clones can serve as part of a backup strategy, providing
   point-in-time copies.
5. **Efficient Storage Utilization**: Using COW technology, clones initially share
   blocks with the source volume, consuming minimal additional storage.
6. **KubeVirt/CDI Support**: This feature would significantly improve the experience
   of running KubeVirt/CDI workloads, enabling more efficient VM management.

## Goals

- Implement volume cloning using copy-on-write (COW) technology.
- Provide a straightforward API for creating, using, and managing volume clones.
- Ensure clones function independently from their source volumes.
- Implement cloning with minimal performance impact on both source and cloned volumes.
- Support cloning both in-use and idle volumes.

## Non-Goals

- This proposal does not implement a mechanism for synchronizing or reconciling
  changes between source and cloned volumes.
- This proposal does not implement a feature to convert a clone into a snapshot or
  vice versa.

## Proposal

### Key Concepts

1. **Volume Clone**: A copy of an existing volume that can be used independently of
   the source volume.
2. **Copy-on-Write (COW)**: A storage efficiency technique where the clone initially
   shares data blocks with the source volume and only allocates new blocks when data
   is modified.
3. **Implementation Approach**: Volume cloning will be implemented as a three-step
   process:
   - Create a snapshot of the source volume
   - Create a new volume by restoring from the snapshot
   - Delete the snapshot (optionally kept for backup/rollback needs)

### Workflow

1. **Clone Creation**:
   - User requests a clone of an existing volume by creating a new PVC with a
     dataSource field pointing to the source PVC.
   - The CSI driver detects this request and implements the cloning operation.
   - In the backend, the system creates a temporary snapshot of the source volume.
   - The system creates a new volume by restoring from the snapshot.
   - The system automatically cleans up the temporary snapshot after successful clone
     creation.
   - The clone is ready for use as an independent volume.

2. **API and User Interface**:
   - Implement the standard Kubernetes CSI cloning interface using the PVC dataSource
     field.
   - No additional Custom Resources are required, maintaining compatibility with
     industry standards.
   - Support key cloning parameters through the standard Kubernetes objects, including:
     - Source volume reference (via dataSource)
     - Clone capacity (via PVC storage request)
     - Storage class and access modes (via standard PVC fields)

3. **Storage Class and PVC Integration**:
   - Follow the standard Kubernetes approach for volume cloning:

```yaml
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: clone-pvc
spec:
  dataSource:
    name: source-pvc
    kind: PersistentVolumeClaim
  accessModes:
    - ReadWriteOnce
  resources:
    requests:
      storage: 10Gi
  storageClassName: mayastor-csi
```

## User Stories

1. **Story 1**: As a developer, I want to create a clone of a production database
   volume to use in my development environment without affecting production data.

2. **Story 2**: As a system administrator, I want to create clones of volumes to
   provision multiple identical environments quickly.

3. **Story 3**: As a data scientist, I want to clone a dataset volume to perform
   analysis without modifying the original data.

4. **Story 4**: As a DevOps engineer, I want to integrate volume cloning into my
   CI/CD pipeline to create isolated test environments for each build.

5. **Story 5**: As a KubeVirt user, I want to efficiently clone VM volumes to create
   new instances without duplicating the entire data set.

## Implementation Details

### Design

- **Backend Implementation**:
  - Use existing snapshot and restore APIs.
  - Implement the CSI Controller's CloneVolume functionality to handle PVC
    dataSource-based cloning requests.
  - Handle temporary snapshot creation and cleanup internally.

- **Control Plane Changes**:
  - Extend the CSI controller to implement volume cloning using the standard
    Kubernetes approach.
  - Add validation logic for clone operations (e.g., capacity checks).
  - Ensure proper status and event reporting for cloning operations.

- **Data Plane Changes**:
  - Minimal changes required since cloning uses existing snapshot and restore
    functionality.
  - Potentially optimize the snapshot-restore-delete sequence for cloning operations.

- **Clone Lifecycle**:
  - A clone operates as an independent volume after creation.
  - Deleting a source volume will not affect its clones.
  - Clone capacity can be equal to or greater than the source volume.

### Components to Update

- **CSI Driver**:
  - Extend to support volume cloning via DataSource.
  - Advertise the volume cloning capability by adding the `CLONE_VOLUME` controller
    capability in the CSI driver's `ControllerGetCapabilities` response.
  - Implement the `CSI_SPEC_VOLUME_CLONING` feature gate to enable volume cloning
    functionality.
- **Control Plane API**: Add backend support for the cloning operations.
- **CLI Tools**: Add commands for clone operations if needed for convenience.

## Testing

- Create a clone of an idle volume and verify it contains identical data.
- Create a clone of an in-use volume and verify data consistency.
- Verify independence of clones by modifying source and clone volumes and ensuring
  changes don't affect each other.
- Verify clone performance matches expected metrics.
- Test clone operations on volumes of various sizes and with different workloads.
- Verify cloning works correctly with volumes of different storage classes.
- Create a clone with larger capacity than the source and verify it can use the
  additional space.
- Test error conditions (e.g., insufficient storage pool capacity, invalid parameters).
