use crate::error::BgError;

/// Transport layer used for synchronizing data between Worktree peers.
///
/// Each variant encapsulates the connection address/endpoint string for
/// its respective protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    /// QUIC-based transport — low-latency, multiplexed, encrypted by default.
    /// The inner string is the endpoint address (e.g. `"127.0.0.1:4433"`).
    Quic(String),

    /// Plain TCP transport — simpler but without built-in encryption.
    /// The inner string is the endpoint address (e.g. `"127.0.0.1:9877"`).
    Tcp(String),
}

impl Transport {
    /// Return the endpoint address string regardless of variant.
    pub fn address(&self) -> &str {
        match self {
            Transport::Quic(addr) => addr,
            Transport::Tcp(addr) => addr,
        }
    }

    /// Return a human-readable protocol name.
    pub fn protocol_name(&self) -> &'static str {
        match self {
            Transport::Quic(_) => "QUIC",
            Transport::Tcp(_) => "TCP",
        }
    }

    /// Establish a connection to the remote endpoint.
    ///
    /// This will negotiate the appropriate protocol handshake and return
    /// once the connection is ready for data transfer.
    pub async fn connect(&self) -> Result<(), BgError> {
        tracing::info!(
            "Connecting via {} to {}",
            self.protocol_name(),
            self.address()
        );

        match self {
            Transport::Quic(addr) => {
                let _ = addr;
                todo!("establish QUIC connection to remote endpoint")
            }
            Transport::Tcp(addr) => {
                let _ = addr;
                todo!("establish TCP connection to remote endpoint")
            }
        }
    }
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.protocol_name(), self.address())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_returns_inner_string() {
        let quic = Transport::Quic("127.0.0.1:4433".to_string());
        assert_eq!(quic.address(), "127.0.0.1:4433");

        let tcp = Transport::Tcp("10.0.0.1:9877".to_string());
        assert_eq!(tcp.address(), "10.0.0.1:9877");
    }

    #[test]
    fn protocol_name_is_correct() {
        assert_eq!(Transport::Quic(String::new()).protocol_name(), "QUIC");
        assert_eq!(Transport::Tcp(String::new()).protocol_name(), "TCP");
    }

    #[test]
    fn display_format() {
        let t = Transport::Quic("localhost:4433".to_string());
        assert_eq!(t.to_string(), "QUIC(localhost:4433)");
    }
}
