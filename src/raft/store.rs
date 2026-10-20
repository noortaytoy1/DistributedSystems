//! In-memory Raft log and state machine storage for openraft (storage API v2).
//!
//! Owner: A (Raft and replication).

// TODO(A): `LogStore` (RaftLogStorage + RaftLogReader) and `StateMachineStore`
// (RaftStateMachine + RaftSnapshotBuilder) wrapping `state::CloudState`.
