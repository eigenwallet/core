use std::{any::Any, convert::Infallible, marker::PhantomData, sync::Arc};

use tokio::sync::mpsc;

pub struct Context {
    monero: Component<MoneroWallet>,
    config: String,
}

pub struct Component<T> {
    inner: Arc<dyn Any + 'static + Send + Sync>,
    /// Event channels to listen for udpates, e.g. wait_until_ready()
    sender: mpsc::Sender<()>,
    receiver: mpsc::Receiver<()>,
    /// Needed to make sure we always know the type T
    _marker: PhantomData<T>,
}

pub type MoneroWallet = Arc<String>;

impl Context {
    fn config(&self) -> String {
        self.config.clone()
    }

    async fn build(config: impl Into<String>) -> Self {
        Self {
            monero: Component::new(),
            config: config.into(),
        }
    }

    async fn init_monero(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl<T: 'static + Send + Sync> Component<T> {
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel(1);

        Self {
            inner: Arc::new(()),
            sender,
            receiver,
            _marker: PhantomData,
        }
    }

    /// Try to get the component value.
    async fn try_get(&self) -> Option<Arc<T>> {
        self.inner.clone().downcast::<T>().ok()
    }

    /// Set the component value.
    async fn set(&mut self, value: T) {
        self.inner = Arc::new(value);
        let _ = self.sender.send(()).await;
    }

    /// Wait until the component is ready.
    async fn wait_until_ready(&mut self) -> Result<Arc<T>, Infallible> {
        while let Some(_) = self.receiver.recv().await {
            // Wait for the component to be ready
        }

        let Some(value) = self.try_get() else {
        loop {}
        };

        Ok(value)
    }
}

async fn get_balance(context: Context) -> anyhow::Result<String> {
    let monero = context.monero.wait_until_ready()
    
    Ok(format!("Monero balance: {}", monero))
}

async fn foo() -> anyhow::Result<()> {
    let context = Context::build("testnet").await;

    Ok(())
}
