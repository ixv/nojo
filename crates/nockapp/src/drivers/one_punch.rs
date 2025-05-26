use crate::nockapp::driver::*;
use crate::nockapp::wire::Wire;
use crate::nockapp::NockAppError;
use crate::noun::slab::NounSlab;
use either::Either::{self, Left, Right};
use nockvm::noun::D;
use nockvm_macros::tas;
use tracing::{debug, error};

pub enum OnePunchWire {
    Poke,
}

impl Wire for OnePunchWire {
    const VERSION: u64 = 1;
    const SOURCE: &'static str = "one-punch";
}

pub fn one_punch_man(data: NounSlab, op: Operation) -> IODriverFn {
    make_driver(|handle| async move {
        let wire = OnePunchWire::Poke.to_wire();
        let result = match op {
            Operation::Poke => Left(handle.poke(wire, data).await?),
            Operation::Peek => {
                debug!("poke_once_driver: peeking with {:?}", data);
                Right(handle.peek(data).await?)
            }
        };

        tokio::select! {
            res = handle_result(result, &op) => res,
            eff = handle.next_effect() => {
                handle_effect(eff, &handle).await
            },
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(600)) => {
                //TODO what is a good timeout for tests?
                debug!("poke_once_driver: no effect received after 10 minutes");
                Err(NockAppError::Timeout)
            }
        }
    })
}
/// Handles the result of a poke or peek operation.
///
/// Poke:
/// - Ack: The poke operation was successful.
/// - Nack: The poke operation failed.
///
/// Peek:
/// - Some(NounSlab): The peek operation was successful and returned a NounSlab.
/// - None: The peek operation failed or returned no result.
///
/// # Arguments
///
/// * `result` - The result of the operation.
/// * `op` - The operation type (Poke or Peek).
///
/// # Returns
///
/// A Result indicating success or failure of the operation.
async fn handle_result(
    result: Either<PokeResult, Option<NounSlab>>,
    op: &Operation,
) -> Result<(), NockAppError> {
    match op {
        Operation::Poke => match result {
            Left(PokeResult::Ack) => {
                debug!("Poke successful");
                Ok(())
            }
            Left(PokeResult::Nack) => {
                debug!("Poke nacked");
                Err(NockAppError::PokeFailed)
            }
            Right(_) => {
                debug!("Unexpected result for poke operation");
                Err(NockAppError::UnexpectedResult)
            }
        },
        Operation::Peek => match result {
            Left(_) => {
                debug!("Unexpected result for peek operation");
                Err(NockAppError::UnexpectedResult)
            }
            Right(Some(peek_result)) => {
                debug!("Peek result: {:?}", peek_result);
                Ok(())
            }
            Right(_) => {
                debug!("Peek returned no result");
                Err(NockAppError::PeekFailed)
            }
        },
    }
}

/// Handles effects from the kernel.
///
/// # Arguments
///
/// * `eff` - The effect produced by the kernel.
/// * `_handle` - The NockAppHandle (unused in this implementation).
///
/// # Returns
///
/// A Result indicating success or failure of handling the effect.
async fn handle_effect(
    eff: Result<NounSlab, NockAppError>,
    _handle: &NockAppHandle,
) -> Result<(), NockAppError> {
    let eff = eff?;
    debug!("poke_once_driver: effect received: {:?}", eff);

    // Split out root bindings so they don't get dropped early
    let root = unsafe { eff.root() };
    debug!("poke_once_driver: root: {:?}", root);

    if root.is_atom() {
        error!("No effects were returned from one-shot poke.");
        return Err(NockAppError::PokeFailed);
    }

    let effect_cell = root.as_cell().unwrap_or_else(|err| {
        panic!(
            "Panicked with {err:?} at {}:{} (git sha: {:?})",
            file!(),
            line!(),
            option_env!("GIT_SHA")
        )
    });
    if unsafe { effect_cell.head().raw_equals(&D(tas!(b"npc"))) } {
        let npc_effect = effect_cell.tail();
        if let Ok(npc_effect_cell) = npc_effect.as_cell() {
            match npc_effect_cell
                .head()
                .as_atom()
                .unwrap_or_else(|err| {
                    panic!(
                        "Panicked with {err:?} at {}:{} (git sha: {:?})",
                        file!(),
                        line!(),
                        option_env!("GIT_SHA")
                    )
                })
                .as_u64()
                .expect("Failed to convert to u64")
            {
                tas!(b"gossip") => {
                    // Ignore gossip data
                    debug!("Ignoring gossip data");
                }
                tas!(b"request") => {
                    debug!("Processing request effect");
                    let request_data = npc_effect_cell.tail();
                    debug!("Request data: {:?}", request_data);
                    // handle.poke(create_response(request_data)).await?;
                }
                _ => debug!("Received unknown npc effect"),
            }
        }
    }
    Ok(())
}
