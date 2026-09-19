#[derive(Debug, PartialEq, Eq)]
pub enum ScopedKeyError {
    EmptyKey,
    AbsolutePath,
    PathTraversal,
    OutsideScope,
}

impl std::fmt::Display for ScopedKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyKey => write!(f, "empty key"),
            Self::AbsolutePath => write!(f, "absolute path not allowed"),
            Self::PathTraversal => write!(f, "path traversal not allowed"),
            Self::OutsideScope => write!(f, "key outside allowed scope"),
        }
    }
}

impl std::error::Error for ScopedKeyError {}

#[derive(Debug)]
pub enum PipelineError {
    S3Download(String),
    Md5Mismatch {
        expected: String,
        actual: String,
    },
    UnzipFailed(String),
    ConfigYamlInvalid(String),
    DockerFailed {
        exit_code: i32,
        logs_tail: String,
    },
    ArtifactUpload(String),
    ReportFailed(String),
    /// GPU orchestrator recebeu imagem mock — guarda anti-mock (D2).
    GpuImageGuard {
        image: String,
    },
    /// Erro genérico (sem variante específica).
    Other(String),
    /// Daemon de difusão falhou ao subir (D1).
    DaemonLaunchFailed(String),
    /// Daemon de difusão não respondeu health a tempo (D1).
    DaemonHealthTimeout(String),
    /// Daemon de difusão busy após múltiplas tentativas (D1).
    DaemonBusy,
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::S3Download(e) => write!(f, "S3 download failed: {e}"),
            Self::Md5Mismatch { expected, actual } => {
                write!(f, "MD5 mismatch: expected {expected}, got {actual}")
            }
            Self::UnzipFailed(e) => write!(f, "unzip failed: {e}"),
            Self::ConfigYamlInvalid(e) => write!(f, "config.yaml invalid: {e}"),
            Self::DockerFailed {
                exit_code,
                logs_tail,
            } => {
                write!(f, "trainer failed (exit {exit_code}):\n{logs_tail}")
            }
            Self::ArtifactUpload(e) => write!(f, "artifact upload failed: {e}"),
            Self::ReportFailed(e) => write!(f, "report failed: {e}"),
            Self::GpuImageGuard { image } => {
                write!(
                    f,
                    "GPU orchestrator requires GPU trainer image (TRAINER_IMAGE={image} → :gpu)"
                )
            }
            Self::Other(e) => write!(f, "{e}"),
            Self::DaemonLaunchFailed(e) => write!(f, "daemon launch failed: {e}"),
            Self::DaemonHealthTimeout(e) => write!(f, "daemon health timeout: {e}"),
            Self::DaemonBusy => write!(f, "daemon busy after retries"),
        }
    }
}

impl std::error::Error for PipelineError {}
