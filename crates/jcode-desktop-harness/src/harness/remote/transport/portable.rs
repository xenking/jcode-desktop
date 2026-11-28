//! Platforms without shared OpenSSH control sockets keep the SDK's isolated
//! transport behavior and security defaults.

#[derive(Clone, Default)]
pub(in crate::harness) struct RemoteTransports;

impl RemoteTransports {
    pub(in crate::harness) fn connect(
        &self,
        host: &str,
    ) -> jcode_sdk::Result<jcode_sdk::JcodeClient> {
        self.connect_with_progress(host, &mut |_| {})
            .map(|(client, _)| client)
    }

    pub(in crate::harness) fn connect_with_progress(
        &self,
        host: &str,
        progress: &mut dyn FnMut(&str),
    ) -> jcode_sdk::Result<(jcode_sdk::JcodeClient, bool)> {
        if let Some(destination) = crate::managed_cloud::Destination::parse(host) {
            return crate::managed_cloud::connect_with_sync_status(
                progress,
                format!("jcode-desktop-remote/{}", crate::client_version()),
                |account, key| crate::managed_cloud::request_connect(account, destination, key),
                |options, _, _| jcode_sdk::JcodeClient::connect_ssh(options),
            )
            .map_err(|message| {
                jcode_sdk::Error::new(jcode_sdk::ErrorKind::ConnectFailed, message)
            });
        }
        let host = crate::remote_targets::validate_host(host).map_err(|message| {
            jcode_sdk::Error::new(jcode_sdk::ErrorKind::InvalidOption, message)
        })?;
        jcode_sdk::JcodeClient::connect_ssh(jcode_sdk::SshConnectOptions {
            // Temporarily disabled until inline tool responses have broader testing.
            supports_user_response: false,
            supports_applets: true,
            client_name: format!("jcode-desktop-remote/{}", crate::client_version()),
            connect_timeout: std::time::Duration::from_secs(20),
            request_timeout: Some(std::time::Duration::from_secs(30)),
            ..jcode_sdk::SshConnectOptions::new(host)
        })
        .map(|client| (client, false))
    }
}
