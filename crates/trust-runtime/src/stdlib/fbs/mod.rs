//! Standard function blocks (TON, CTU, etc.).

#![allow(missing_docs)]

mod bistable;
mod counters;
mod instance;
mod registry;
mod state;
mod timers;
mod triggers;

pub use bistable::{Rs, Sr};
pub use counters::{CounterOutput, CounterUpDownOutput, Ctd, Ctu, Ctud};
pub use registry::{builtin_kind, standard_function_blocks, BuiltinFbKind};
pub use timers::{TimerOutput, Tof, Ton, Tp};
pub use triggers::{FTrig, RTrig};

use crate::error::RuntimeError;
use crate::eval::EvalContext;
use crate::memory::InstanceId;

pub fn execute_builtin(
    ctx: &mut EvalContext<'_>,
    instance_id: InstanceId,
    kind: BuiltinFbKind,
) -> Result<(), RuntimeError> {
    match kind {
        BuiltinFbKind::Rs => bistable::exec_rs(ctx, instance_id),
        BuiltinFbKind::Sr => bistable::exec_sr(ctx, instance_id),
        BuiltinFbKind::RTrig => triggers::exec_r_trig(ctx, instance_id),
        BuiltinFbKind::FTrig => triggers::exec_f_trig(ctx, instance_id),
        BuiltinFbKind::Ctu => counters::exec_ctu(ctx, instance_id),
        BuiltinFbKind::Ctd => counters::exec_ctd(ctx, instance_id),
        BuiltinFbKind::Ctud => counters::exec_ctud(ctx, instance_id),
        BuiltinFbKind::Tp => timers::exec_tp(ctx, instance_id),
        BuiltinFbKind::Ton => timers::exec_ton(ctx, instance_id),
        BuiltinFbKind::Tof => timers::exec_tof(ctx, instance_id),
    }
}
