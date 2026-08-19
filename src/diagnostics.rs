#[derive(Debug, Clone, Copy)]
pub enum DiagnosticStatus {
    Ok,
    Warn,
    Fail,
    Info,
}

impl DiagnosticStatus {
    fn marker(self) -> &'static str {
        match self {
            Self::Ok => "✓",
            Self::Warn => "⚠",
            Self::Fail => "✗",
            Self::Info => "◇",
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum DiagnosticCode {
    ConfigResolved,
    DriverVerified,
    SecretResolved,
    SshBastionReachable,
    SshBastionUnreachable,
    SshBastionUnresolved,
    SshIdentityMissing,
    SshTunnelAttempt,
    SshTunnelFailed,
    PostgresReachable,
    PostgresUnreachable,
    SidecarStartAttempt,
    SidecarBackendOk,
    SidecarConnectionFailed,
    BackendVerificationOk,
    BackendVerificationFailed,
    AllChecksPassed,
    ConnectionLost,
    SshTunnelRecoveryAttempt,
    JdbcReconnectAttempt,
    SidecarRestartAttempt,
    RecoveryOk,
    RecoveryFailed,
}

impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        const NAMES: [&str; 23] = [
            "SAFESELECT_CONFIG_RESOLVED",
            "SAFESELECT_DRIVER_VERIFIED",
            "SAFESELECT_SECRET_RESOLVED",
            "SAFESELECT_SSH_BASTION_REACHABLE",
            "SAFESELECT_SSH_BASTION_UNREACHABLE",
            "SAFESELECT_SSH_BASTION_UNRESOLVED",
            "SAFESELECT_SSH_IDENTITY_MISSING",
            "SAFESELECT_SSH_TUNNEL_ATTEMPT",
            "SAFESELECT_SSH_TUNNEL_FAILED",
            "SAFESELECT_POSTGRES_REACHABLE",
            "SAFESELECT_POSTGRES_UNREACHABLE",
            "SAFESELECT_SIDECAR_START_ATTEMPT",
            "SAFESELECT_SIDECAR_BACKEND_OK",
            "SAFESELECT_SIDECAR_CONNECTION_FAILED",
            "SAFESELECT_BACKEND_VERIFICATION_OK",
            "SAFESELECT_BACKEND_VERIFICATION_FAILED",
            "SAFESELECT_ALL_CHECKS_PASSED",
            "SAFESELECT_CONNECTION_LOST",
            "SAFESELECT_SSH_TUNNEL_RECOVERY_ATTEMPT",
            "SAFESELECT_JDBC_RECONNECT_ATTEMPT",
            "SAFESELECT_SIDECAR_RESTART_ATTEMPT",
            "SAFESELECT_RECOVERY_OK",
            "SAFESELECT_RECOVERY_FAILED",
        ];
        NAMES[self as usize]
    }
}

pub fn line(status: DiagnosticStatus, code: DiagnosticCode, message: impl AsRef<str>) -> String {
    format!(
        "  {} [{}] {}",
        status.marker(),
        code.as_str(),
        message.as_ref()
    )
}

pub fn print(status: DiagnosticStatus, code: DiagnosticCode, message: impl AsRef<str>) {
    println!("{}", line(status, code, message));
}
