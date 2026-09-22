use super::snapshot::{Snapshot, Store};
use accesskit::TreeUpdate;
use ipc_rpc::{ConnectionKey, IpcRpcClient, IpcRpcServer};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};
use tokio::{
    runtime::Runtime,
    sync::{Mutex, oneshot},
};

/// Overrides the discovery file used to connect the separately launched MCP bridge to the TUI.
///
/// Tests and custom environments can use it to isolate their endpoint instead of sharing the
/// default discovery file in the platform cache directory.
const ENDPOINT_ENV: &str = "MOLERAT_MCP_ENDPOINT_FILE";

/// Resolves the shared endpoint discovery path.
fn endpoint_path() -> PathBuf {
    if let Some(path) = std::env::var_os(ENDPOINT_ENV) {
        return PathBuf::from(path);
    }
    let cache = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
    cache.join(env!("CARGO_PKG_NAME")).join("mcp.key")
}

/// Messages exchanged between the running application and the MCP facade.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
enum Message {
    /// Requests the latest semantic UI snapshot from the running TUI application.
    SnapshotRequest,
    /// Returns the snapshot as JSON, or an error message if it could not be produced.
    ///
    /// The snapshot is encoded as JSON because `ipc-rpc`'s positional binary format does not
    /// support the optional fields omitted by [`Snapshot`] during serialization.
    SnapshotResponse(Result<String, String>),
}

/// Application-side endpoint serving the current semantic snapshot.
#[derive(Debug)]
pub(crate) struct Endpoint {
    store: Store,
    shutdown: Option<oneshot::Sender<()>>,
    worker: Option<JoinHandle<()>>,
    path: PathBuf,
    key: ConnectionKey,
}

impl Endpoint {
    /// Starts an endpoint at the default discovery path.
    pub(crate) fn spawn(application: &str) -> io::Result<Self> {
        Self::spawn_at(application, endpoint_path())
    }

    /// Starts an endpoint at a specific discovery path.
    pub(super) fn spawn_at(application: &str, path: PathBuf) -> io::Result<Self> {
        let store = Store::new(application);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let worker_store = store.clone();
        let worker_path = path.clone();
        let worker = thread::Builder::new()
            .name("molerat-ipc".into())
            .spawn(move || {
                if let Err(error) = run_endpoint(worker_store, worker_path, &ready_tx, shutdown_rx)
                {
                    let _ = ready_tx.send(Err(error));
                }
            })?;
        let setup = ready_rx
            .recv()
            .map_err(|_| io::Error::other("Molerat IPC setup thread stopped unexpectedly"))?;
        let key = match setup {
            Ok(key) => key,
            Err(error) => {
                let _ = worker.join();
                return Err(error);
            }
        };

        Ok(Self {
            store,
            shutdown: Some(shutdown_tx),
            worker: Some(worker),
            path,
            key,
        })
    }

    /// Applies an AccessKit update to the exposed semantic tree.
    pub(crate) fn update(&self, update: &TreeUpdate) {
        self.store.update(update);
    }
}

/// Runs the IPC server until the application endpoint is dropped.
fn run_endpoint(
    store: Store,
    path: PathBuf,
    ready: &mpsc::SyncSender<io::Result<ConnectionKey>>,
    shutdown: oneshot::Receiver<()>,
) -> io::Result<()> {
    let runtime = Runtime::new()?;
    runtime.block_on(async move {
        let (key, server) = IpcRpcServer::initialize_server(move |message| {
            let store = store.clone();
            async move {
                match message {
                    Message::SnapshotRequest => Some(Message::SnapshotResponse(
                        serde_json::to_string(&store.snapshot()).map_err(|error| error.to_string()),
                    )),
                    Message::SnapshotResponse(_) => None,
                }
            }
        })
        .await
        .map_err(io::Error::other)?;

        publish_key(&path, &key)?;
        ready
            .send(Ok(key.clone()))
            .map_err(|_| io::Error::other("Molerat IPC setup was cancelled"))?;

        let _ = shutdown.await;
        if !server.client_connected() {
            let _ = tokio::time::timeout(Duration::from_secs(1), async {
                let client = IpcRpcClient::<Message>::initialize_client(key, |_| async { None })
                    .await
                    .map_err(io::Error::other)?;
                server
                    .wait_for_client_to_connect()
                    .await
                    .map_err(io::Error::other)?;
                drop(client);
                server
                    .wait_for_client_to_disconnect()
                    .await
                    .map_err(io::Error::other)
            })
            .await;
        }
        drop(server);
        Ok(())
    })
}

/// Atomically publishes a private ipc-rpc connection key.
fn publish_key(path: &Path, key: &ConnectionKey) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "MCP endpoint path has no parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(key.to_string().as_bytes())?;
        file.sync_all()?;
    }
    #[cfg(not(unix))]
    fs::write(&temporary, key.to_string())?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)
}

impl Drop for Endpoint {
    /// Closes the IPC server and removes its discovery file.
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }

        let owns_endpoint =
            fs::read_to_string(&self.path).is_ok_and(|key| key.trim() == self.key.to_string());
        if owns_endpoint {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Client used by the stdio MCP facade to query the live application.
#[derive(Clone, Debug)]
pub(super) struct Client {
    path: PathBuf,
    connection: Arc<Mutex<Option<Connection>>>,
}

#[derive(Debug)]
struct Connection {
    key: String,
    client: IpcRpcClient<Message>,
}

impl Client {
    /// Creates a client for the default discovery path.
    pub(super) fn new() -> Self {
        Self::at_path(endpoint_path())
    }

    /// Creates a client for a specific discovery path.
    pub(super) fn at_path(path: PathBuf) -> Self {
        Self {
            path,
            connection: Arc::new(Mutex::new(None)),
        }
    }

    /// Fetches the latest semantic snapshot from the live application.
    pub(super) async fn snapshot(&self) -> io::Result<Snapshot> {
        let key = fs::read_to_string(&self.path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "Molerat is not available; start an MCP-enabled application (looked for {}): \
                     {error}",
                    self.path.display()
                ),
            )
        })?;
        let key = key.trim().to_owned();
        let mut connection = self.connection.lock().await;
        if connection
            .as_ref()
            .is_none_or(|connection| connection.key != key)
        {
            let mut client =
                IpcRpcClient::initialize_client(ConnectionKey::from(key.clone()), |_| async {
                    None
                })
                .await
                .map_err(io::Error::other)?;
            let validation = client.schema_validated().await.map_err(io::Error::other)?;
            if !validation.is_success() {
                return Err(io::Error::other(format!(
                    "Molerat IPC schema validation failed: {validation:?}"
                )));
            }
            *connection = Some(Connection { key, client });
        }

        let result = match connection.as_ref() {
            Some(connection) => connection.client.send(Message::SnapshotRequest).await,
            None => {
                return Err(io::Error::other(
                    "Molerat IPC connection was not initialized",
                ));
            }
        };
        match result {
            Ok(Message::SnapshotResponse(Ok(snapshot))) => serde_json::from_str(&snapshot)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            Ok(Message::SnapshotResponse(Err(error))) => Err(io::Error::other(error)),
            Ok(Message::SnapshotRequest) => Err(io::Error::other(
                "Molerat returned an unexpected IPC response",
            )),
            Err(error) => {
                *connection = None;
                Err(io::Error::other(error))
            }
        }
    }
}
