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
        match self {
            Self::ConfigResolved => "SAFESELECT_CONFIG_RESOLVED",
            Self::DriverVerified => "SAFESELECT_DRIVER_VERIFIED",
            Self::SecretResolved => "SAFESELECT_SECRET_RESOLVED",
            Self::SshBastionReachable => "SAFESELECT_SSH_BASTION_REACHABLE",
            Self::SshBastionUnreachable => "SAFESELECT_SSH_BASTION_UNREACHABLE",
            Self::SshBastionUnresolved => "SAFESELECT_SSH_BASTION_UNRESOLVED",
            Self::SshIdentityMissing => "SAFESELECT_SSH_IDENTITY_MISSING",
            Self::SshTunnelAttempt => "SAFESELECT_SSH_TUNNEL_ATTEMPT",
            Self::SshTunnelFailed => "SAFESELECT_SSH_TUNNEL_FAILED",
            Self::PostgresReachable => "SAFESELECT_POSTGRES_REACHABLE",
            Self::PostgresUnreachable => "SAFESELECT_POSTGRES_UNREACHABLE",
            Self::SidecarStartAttempt => "SAFESELECT_SIDECAR_START_ATTEMPT",
            Self::SidecarBackendOk => "SAFESELECT_SIDECAR_BACKEND_OK",
            Self::SidecarConnectionFailed => "SAFESELECT_SIDECAR_CONNECTION_FAILED",
            Self::BackendVerificationOk => "SAFESELECT_BACKEND_VERIFICATION_OK",
            Self::BackendVerificationFailed => "SAFESELECT_BACKEND_VERIFICATION_FAILED",
            Self::AllChecksPassed => "SAFESELECT_ALL_CHECKS_PASSED",
            Self::ConnectionLost => "SAFESELECT_CONNECTION_LOST",
            Self::SshTunnelRecoveryAttempt => "SAFESELECT_SSH_TUNNEL_RECOVERY_ATTEMPT",
            Self::JdbcReconnectAttempt => "SAFESELECT_JDBC_RECONNECT_ATTEMPT",
            Self::SidecarRestartAttempt => "SAFESELECT_SIDECAR_RESTART_ATTEMPT",
            Self::RecoveryOk => "SAFESELECT_RECOVERY_OK",
            Self::RecoveryFailed => "SAFESELECT_RECOVERY_FAILED",
        }
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
