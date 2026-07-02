#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransferMode {
    Buffered,
    ZeroCopy,
}

impl Default for TransferMode {
    fn default() -> Self {
        TransferMode::Buffered
    }
}
