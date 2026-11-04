//! The one interface the application layer may use (D156, D211).
//!
//! Lecture 9 asks for the middleware and the application to be kept apart in the implementation
//! as well as the design, on the cloud side and on the client side. This file is that line.
//! Application code takes what it needs from the layer below here and nowhere else, and
//! `tests/layers.rs` fails if it reaches past. Only re-exports live here, so this file does nothing
//! by itself.
//!
//! ```text
//!               a client PC                       a cloud node
//!
//! application   the users: bin/loadgen.rs,        the incident service: node/service.rs
//!               bin/frontend.rs                   its rules: node/release.rs, planner.rs, state.rs
//!                  |                                 |
//!                  | client.submit(incident)         | decide_executor(ctx, request)
//!                  |                                 | ctx.group.send_ordered(command)
//! =================|======= src/middleware.rs =======|=====================================
//!                  v                                 v
//! middleware    MeshClient, client.rs             CloudGroup, node/group.rs
//!                  submit makes one call to          bidding, the detector, takeover,
//!                  group_send, which makes           rejoin, degraded mode
//!                  one copy per node                 and a busy leader stepping aside
//!                  |                                 |
//! libraries     tarpc over UDP                    tarpc over TCP, openraft
//! ```
//!
//! - **Client side.** User code makes one call, [`MeshClient::submit`], and never names a node.
//!   Behind it, the private `MeshClient::group_send` gives each cloud node its own copy. That is
//!   the group send (D126, D210): the client middleware's send to all of the cloud nodes, which the
//!   application does not know happens. The retries with the same ID and the first answer winning
//!   sit behind the same call. [`control`] is how the web page reads a node's status, as `ctl`
//!   does.
//! - **Cloud side.** The incident service asks who plans a request and commits a result through
//!   [`CloudGroup::send_ordered`]. Who else is alive, how a winner was chosen and how the result
//!   reached the other replicas are the middleware's business. The middleware calls up to the
//!   service for each request. [`NodeContext`] is shared by every part, so the compiler alone would
//!   let the service read `ctx.consensus`: `tests/layers.rs` is what stops that.
//!
//! Beside both layers: `proto.rs`, the contract, and `config.rs` and `metrics.rs`, which everyone
//! uses. Application code names those directly.
//!
//! Owner: shared (A, C and F).

// The client side: what user code calls.
pub use crate::client::{ClientError, MeshClient, Submitted, control};

// The cloud side: what the incident service calls.
pub use crate::node::group::{CloudGroup, View};
pub use crate::node::transport::ReplyTx;
pub use crate::node::workers::Overloaded;
pub(crate) use crate::node::{Decision, decide_executor};
pub use crate::node::{NodeContext, degraded};
