use bitcoincore_rpc::{Auth, Client, Result, RpcApi};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

/// Bitcoind RPC client configuration, must use `check_node` feature
pub struct ClientConfig {
    ///cookie_auth_path takes precedence over user/passwd authentication.
    pub cookie_auth_path: Option<PathBuf>,
    pub ip_addr: String,
    pub user: String,
    pub passwd: String,
}

fn get_client(cfg: &ClientConfig) -> Result<Client> {
    let client = if let Some(path) = &cfg.cookie_auth_path {
        get_client_by_cookie(&cfg.ip_addr, path.clone())?
    } else {
        get_client_by_user_passw(&cfg.ip_addr, cfg.user.clone(), cfg.passwd.clone())?
    };
    Ok(client)
}

fn get_client_by_cookie(ip: &str, path: PathBuf) -> Result<Client> {
    Client::new(ip, Auth::CookieFile(path))
}

fn get_client_by_user_passw(ip: &str, user_name: String, passwd: String) -> Result<Client> {
    Client::new(ip, Auth::UserPass(user_name, passwd))
}

/// Checks bitcoind node state before obtaining ZMQ Messages. Must use `check_node` feature
/// ```
///    let checker = NodeChecker::new(&ClientConfig {
///        cookie_auth_path: None,
///        ip_addr: "localhost".to_string(),
///        user: "my_user".to_string(),
///        passwd: "my_passwd".to_string(),
///    })?;
///
///    let has_index = checker.check_tx_index()?;
///    if !has_index {
///        return Err(anyhow!(
///            "bitcoind must have transactions index enabled, add txindex=1 to bitcoin.conf file"
///        ));
///    }
///
///    println!("Waiting to node Ok");
///    checker.wait_till_node_ok(2, Duration::from_secs(5))?;
///    println!("Node Ok");
/// ```
pub struct NodeChecker {
    pub client: Client,
}

impl NodeChecker {
    /// Get a [`NodeChecker`] from a [`ClientConfig`]
    pub fn new(cfg: &ClientConfig) -> Result<NodeChecker> {
        Ok(NodeChecker {
            client: get_client(cfg)?,
        })
    }

    /// Checks if bitcoind node has at least `min_out_connections`
    pub fn check_network_peers(&self, min_out_connections: usize) -> Result<bool> {
        let gniresut = self.client.get_network_info()?;
        Ok(gniresut.network_active && gniresut.connections_out.unwrap_or(0) >= min_out_connections)
    }

    /// Checks if bitcoind node has downloaded all blockchain
    pub fn check_blockchain_in_sync(&self) -> Result<bool> {
        let gbciresult = self.client.get_blockchain_info()?;
        Ok(!gbciresult.initial_block_download && gbciresult.blocks == gbciresult.headers)
    }

    /// Checks if tx index is enabled and in sync in the bitcoind node
    pub fn check_tx_index(&self) -> Result<bool> {
        let giiresult = self.client.get_index_info()?;
        if giiresult.txindex.is_some() {
            return Ok(giiresult.txindex.unwrap().synced);
        }
        Ok(false)
    }

    /// Wait until bitcoind node has at least `min_out_connections` and has downloaded all blockchain,
    /// Check is every sleep_time duration.
    pub fn wait_till_node_ok(
        &self,
        min_out_connections: usize,
        sleep_time: Duration,
    ) -> Result<bool> {
        loop {
            if self.check_network_peers(min_out_connections)? && self.check_blockchain_in_sync()? {
                return Ok(true);
            }
            thread::sleep(sleep_time);
        }
    }
}
