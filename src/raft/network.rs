//! openraft over our transport: Raft RPCs travel as `PeerBody::Raft` frames on the peer port.
//!
//! Owner: A (Raft and replication).

// TODO(A): `RaftNetworkFactory` / `RaftNetwork` over `node::transport::PeerSet`, and the
// receiving side that hands incoming RPCs to openraft.
