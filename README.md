# bitcoincore-zmqsequence
A ZeroMQ pubsequence listener.
Allows keeping a synchronized copy of a Bitcoin Core node's mempool.
Example:
```
use anyhow::Result;
use bitcoincore_zmqsequence::ZmqSeqListener;
use ctrlc;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use url::Url;


fn main() -> Result<()> {
    let checker = NodeChecker::new(&ClientConfig {
        cookie_auth_path: None,
        ip_addr: "localhost".to_string(),
        user: "anon".to_string(),
        passwd: "anon".to_string(),
    })?;

    println!("Waiting to node Ok");
    checker.wait_till_node_ok(2, false, Duration::from_secs(5))?;
    println!("Node Ok");

    let stop_th = Arc::new(AtomicBool::new(false));
    let stop_th2 = stop_th.clone();
    ctrlc::set_handler(move || stop_th2.store(true, Ordering::SeqCst))?;
    let zmqseqlistener = ZmqSeqListener::start(&Url::from_str("tcp://localhost:29000")?)?;
    while !stop_th.load(Ordering::SeqCst) {
        let kk = zmqseqlistener.rx.recv()?;
        println!("{:?}", kk);
    }
    Ok(())
}
```
