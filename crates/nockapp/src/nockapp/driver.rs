use crate::noun::slab::NounSlab;
use futures::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio::task::JoinSet;
use tracing::instrument;

use super::error::NockAppError;
use super::wire::WireRepr;
use super::NockAppExit;

pub type IODriverFuture = Pin<Box<dyn Future<Output = Result<(), NockAppError>> + Send>>;
pub type IODriverFn = Box<dyn FnOnce(NockAppHandle) -> IODriverFuture>;
pub type TaskJoinSet = JoinSet<Result<(), NockAppError>>;
pub type ActionSender = mpsc::Sender<IOAction>;
pub type ActionReceiver = mpsc::Receiver<IOAction>;
pub type EffectSender = broadcast::Sender<NounSlab>;
pub type EffectReceiver = broadcast::Receiver<NounSlab>;

/// Result of a poke: either Ack if it succeeded or Nack if it failed
#[derive(Debug)]
pub enum PokeResult {
    Ack,
    Nack,
}

pub enum Operation {
    Poke,
    Peek,
}

pub fn make_driver<F, Fut>(f: F) -> IODriverFn
where
    F: FnOnce(NockAppHandle) -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), NockAppError>> + Send + 'static,
{
    Box::new(move |handle| Box::pin(f(handle)))
}

pub struct NockAppHandle {
    pub io_sender: ActionSender,
    pub effect_sender: Arc<EffectSender>,
    pub effect_receiver: Mutex<EffectReceiver>,
    pub exit: NockAppExit,
}

/// IO actions sent between [`NockAppHandle`] and [`crate::NockApp`] over channels.
///
/// Used by [`NockAppHandle`] to send poke/peek requests to [`crate::NockApp`] ,
/// which processes them against the Nock kernel and returns results
/// via oneshot channels.
#[derive(Debug)]
pub enum IOAction {
    /// Poke request to [`crate::NockApp`]
    Poke {
        wire: WireRepr,
        poke: NounSlab,
        ack_channel: oneshot::Sender<PokeResult>,
    },
    /// Peek request to [`crate::NockApp`]
    Peek {
        path: NounSlab,
        result_channel: oneshot::Sender<Option<NounSlab>>,
    },
}

impl NockAppHandle {
    #[tracing::instrument(name = "nockapp::NockAppHandle::send_poke", skip_all)]
    pub async fn send_poke(
        &self,
        ack_channel: oneshot::Sender<PokeResult>,
        wire: WireRepr,
        poke: NounSlab,
    ) -> Result<(), NockAppError> {
        self.io_sender
            .send(IOAction::Poke {
                wire,
                poke,
                ack_channel,
            })
            .await?;
        Ok(())
    }

    #[tracing::instrument(name = "nockapp::NockAppHandle::try_send_poke", skip_all)]
    /// Tries to send a poke. Returns NockAppError::MPSCSendError if the channel is closed. If the channel is full, the result is given back in the Some branch of the Option. If the channel is empty
    pub fn try_send_poke(
        &self,
        ack_channel: oneshot::Sender<PokeResult>,
        wire: WireRepr,
        poke: NounSlab,
    ) -> Result<(), NockAppError> {
        Ok(self.io_sender.try_send(IOAction::Poke {
            wire,
            poke,
            ack_channel,
        })?)
    }

    #[tracing::instrument(name = "nockapp::NockAppHandle::poke", skip_all)]
    pub async fn poke(&self, wire: WireRepr, poke: NounSlab) -> Result<PokeResult, NockAppError> {
        let (ack_channel, ack_future) = oneshot::channel();
        self.send_poke(ack_channel, wire, poke).await?;
        Ok(ack_future.await?)
    }

    // This is still async because we still await the ack future on success.
    #[tracing::instrument(name = "nockapp::NockAppHandle::try_poke", skip_all)]
    pub async fn try_poke(
        &self,
        wire: WireRepr,
        poke: NounSlab,
    ) -> Result<PokeResult, NockAppError> {
        let (ack_channel, ack_future) = oneshot::channel();
        self.try_send_poke(ack_channel, wire, poke)?;
        Ok(ack_future.await?)
    }

    #[tracing::instrument(name = "nockapp::NockAppHandle::try_send_peek", skip_all)]
    pub fn try_send_peek(
        &self,
        path: NounSlab,
        result_channel: oneshot::Sender<Option<NounSlab>>,
    ) -> Result<(), NockAppError> {
        Ok(self.io_sender.try_send(IOAction::Peek {
            path,
            result_channel,
        })?)
    }

    #[tracing::instrument(name = "nockapp::NockAppHandle::send_peek", skip_all)]
    async fn send_peek(
        &self,
        path: NounSlab,
        result_channel: oneshot::Sender<Option<NounSlab>>,
    ) -> Result<(), NockAppError> {
        self.io_sender
            .send(IOAction::Peek {
                path,
                result_channel,
            })
            .await?;
        Ok(())
    }

    #[tracing::instrument(name = "nockapp::NockAppHandle::peek", skip_all)]
    pub async fn peek(&self, path: NounSlab) -> Result<Option<NounSlab>, NockAppError> {
        let (result_channel, result_future) = oneshot::channel();
        self.send_peek(path, result_channel).await?;
        Ok(result_future.await?)
    }

    // Still async because we need to await the result future
    #[tracing::instrument(name = "nockapp::NockAppHandle::try_peek", skip_all)]
    pub async fn try_peek(&self, path: NounSlab) -> Result<Option<NounSlab>, NockAppError> {
        let (result_channel, result_future) = oneshot::channel();
        self.send_peek(path, result_channel).await?;
        Ok(result_future.await?)
    }

    #[instrument(skip(self))]
    pub async fn next_effect(&self) -> Result<NounSlab, NockAppError> {
        let mut effect_receiver = self.effect_receiver.lock().await;
        tracing::debug!("Waiting for recv on next effect");
        Ok(effect_receiver.recv().await?)
    }

    #[instrument(skip(self))]
    pub fn dup(self) -> (Self, Self) {
        let io_sender = self.io_sender.clone();
        let effect_sender = self.effect_sender.clone();
        let effect_receiver = Mutex::new(effect_sender.subscribe());
        let exit = self.exit.clone();
        (
            self,
            NockAppHandle {
                io_sender,
                effect_sender,
                effect_receiver,
                exit,
            },
        )
    }

    #[instrument(skip(self))]
    pub fn clone_io_sender(&self) -> ActionSender {
        self.io_sender.clone()
    }
}
