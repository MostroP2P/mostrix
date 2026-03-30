#[derive(Debug, Clone)]
pub enum NetworkStatus {
    Offline(String),
    Online(String),
}

