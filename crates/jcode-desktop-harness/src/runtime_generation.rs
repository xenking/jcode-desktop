//! Desktop consumes the shared generation verdict rather than guessing from
//! socket files or the location of a previous bundle's bridge.
use jcode_build_support::runtime_generation::{
    self as generation, GenerationVerdict, ImmutableTarget, ServingIdentity,
};
use jcode_sdk::{Error, ErrorKind};
use std::sync::OnceLock;
use std::time::Duration;

fn bundle_target() -> Result<ImmutableTarget, String> {
    static TARGET: OnceLock<Result<ImmutableTarget, String>> = OnceLock::new();
    TARGET
        .get_or_init(|| {
            let cli = crate::platform::companion_executable("jcode");
            let target =
                generation::resolve(generation::DesiredSelection::Path(cli)).map_err(|error| {
                    format!("selected Desktop runtime target is unknown: {error:#}")
                })?;
            #[cfg(not(unix))]
            let target = {
                let mut bridge = generation::pin_path(&crate::platform::companion_executable(
                    "jcode-harness-api-bridge",
                ))
                .map_err(|error| format!("selected Desktop bridge target is unknown: {error:#}"))?;
                bridge.git_hash = target.git_hash;
                bridge
            };
            Ok(target)
        })
        .clone()
}

pub(crate) fn verify_local_bridge(identity: Option<&ServingIdentity>) -> jcode_sdk::Result<()> {
    let target =
        bundle_target().map_err(|reason| Error::new(ErrorKind::HandshakeFailed, reason))?;
    verify_bridge_target(&target, identity)
}

fn verify_bridge_target(
    target: &ImmutableTarget,
    identity: Option<&ServingIdentity>,
) -> jcode_sdk::Result<()> {
    match generation::assess_serving_target(target, identity) {
        GenerationVerdict::Ready(_) => Ok(()),
        GenerationVerdict::Unknown { reason }
        | GenerationVerdict::Pending { reason }
        | GenerationVerdict::Mismatch { reason } => Err(Error::new(
            ErrorKind::HandshakeFailed,
            format!("Desktop bridge generation not confirmed: {reason}"),
        )),
    }
}

pub(crate) fn daemon_allows_bridge_revive() -> bool {
    let socket = jcode_sdk::api::legacy_socket_path();
    let verdict = generation::probe_socket_identity(&socket, Duration::from_millis(200))
        .map(|identity| generation::assess_reconnect(&identity, Duration::from_secs(30)));
    match verdict {
        Ok(GenerationVerdict::Ready(_)) => true,
        other => {
            eprintln!(
                "jcode desktop: bridge revive deferred, daemon generation is not confirmed: {other:?}"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn target() -> ImmutableTarget {
        ImmutableTarget {
            binary_path: PathBuf::from("/fixture/Jcode-new.app/Contents/MacOS/jcode"),
            git_hash: Some("new-hash".into()),
            version_label: None,
            file_identity: None,
            payload_identity: None,
        }
    }

    fn bridge() -> ServingIdentity {
        ServingIdentity {
            pid: 42,
            binary_path: target().binary_path,
            git_hash: "new-hash".into(),
            version_label: None,
            reload_id: None,
            generation: 1,
        }
    }

    #[test]
    fn bridge_left_from_previous_bundle_is_rejected_before_attach() {
        let mut old = bridge();
        old.binary_path = "/fixture/Jcode-old.app/Contents/MacOS/jcode".into();
        let error = verify_bridge_target(&target(), Some(&old)).unwrap_err();
        assert!(error.message.contains("another bundle"));
        assert_eq!(error.kind, ErrorKind::HandshakeFailed);
    }

    #[test]
    fn same_path_with_previous_bundle_hash_is_rejected() {
        let mut old = bridge();
        old.git_hash = "old-hash".into();
        assert!(verify_bridge_target(&target(), Some(&old)).is_err());
        assert!(verify_bridge_target(&target(), Some(&bridge())).is_ok());
    }

    #[test]
    fn legacy_bridge_has_explicit_unknown_reason_not_ready() {
        let error = verify_bridge_target(&target(), None).unwrap_err();
        assert!(error.message.contains("did not report serving identity"));
    }
}
