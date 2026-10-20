//! Raft through openraft 0.9.25: elects the cloud coordinator and orders every command.
//!
//! Node code talks to Raft only through the `Consensus` trait defined here, so the library can
//! be swapped for our own Raft without touching `node/`.
//!
//! Owner: A (Raft and replication).

pub mod network;
pub mod store;

use std::io::Cursor;

use openraft::{BasicNode, TokioRuntime};

use crate::proto::{Command, NodeId, Outcome};

openraft::declare_raft_types!(
    /// openraft type configuration for RescueMesh.
    pub TypeConfig:
        D = Command,
        R = Outcome,
        NodeId = NodeId,
        Node = BasicNode,
        Entry = openraft::Entry<TypeConfig>,
        SnapshotData = Cursor<Vec<u8>>,
        AsyncRuntime = TokioRuntime,
);

// TODO(A): `Consensus` trait (propose, status, RPC handling) and the openraft-backed node.
