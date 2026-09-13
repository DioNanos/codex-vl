//! Applies the selected server's socket policy on initial connections and reconnects.

use crate::AppServerTarget;
#[cfg(windows)]
use crate::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY;
use codex_app_server_client::AppServerClient;
#[cfg(windows)]
use codex_app_server_client::RemoteAppServerClient;
#[cfg(windows)]
use codex_app_server_client::RemoteAppServerConnectArgs;
#[cfg(windows)]
use codex_app_server_client::RemoteAppServerEndpoint;
#[cfg(windows)]
use codex_utils_absolute_path::AbsolutePathBuf;

pub(crate) async fn connect(
    target: &AppServerTarget,
    identity_proof: Option<codex_app_server_protocol::IdentityProof>,
) -> color_eyre::Result<AppServerClient> {
    match target {
        AppServerTarget::Embedded => {
            color_eyre::eyre::bail!("embedded sessions have no remote connection")
        }
        #[cfg(windows)]
        AppServerTarget::LocalDaemon {
            endpoint: RemoteAppServerEndpoint::UnixSocket { socket_path },
        } => {
            // Revalidate at the real connection, not just the earlier discovery probe.
            let (socket_path, _directory) =
                codex_uds::validate_private_socket_path(socket_path.as_path())?;
            let args = RemoteAppServerConnectArgs {
                endpoint: RemoteAppServerEndpoint::UnixSocket {
                    socket_path: AbsolutePathBuf::from_absolute_path_checked(socket_path)?,
                },
                client_name: "codex-tui".to_string(),
                client_version: env!("CARGO_PKG_VERSION").to_string(),
                experimental_api: true,
                mcp_server_openai_form_elicitation: false,
                opt_out_notification_methods: Vec::new(),
                channel_capacity: DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
            };
            let app_server = if identity_proof.is_some() {
                RemoteAppServerClient::connect_with_identity(args, identity_proof).await?
            } else {
                RemoteAppServerClient::connect_local_daemon(args).await?
            };
            Ok(AppServerClient::Remote(app_server))
        }
        AppServerTarget::LocalDaemon { endpoint } | AppServerTarget::Remote { endpoint } => {
            crate::connect_remote_app_server_with_identity(endpoint.clone(), identity_proof).await
        }
    }
}
