use crate::errors::ZMQSeqListenerError;
use std::str;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Barrier};
use std::thread;
use std::thread::JoinHandle;

use url::Url;

pub mod errors;

#[cfg(feature = "check_node")]
pub mod check;

type Msg = Vec<Vec<u8>>;

/// Enum with all possible ZMQ messages arriving through the receiver
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MempoolSequence {
    ///A new block has arrived with `block_hash`
    BlockConnection { block_hash: String, zmq_seq: u32 },
    ///A block with `block_hash` has been overridden
    BlockDisconnection { block_hash: String, zmq_seq: u32 },
    ///A tx with `txid` has been removed from the mempool.
    TxRemoved {
        txid: String,
        mp_seq_num: u64,
        zmq_seq: u32,
    },
    ///A tx with `txid` has been added to the mempool.
    TxAdded {
        txid: String,
        mp_seq_num: u64,
        zmq_seq: u32,
    },
    ///An error has ocurred, and must be handled.
    SeqError { error: ZMQSeqListenerError },
    ///First message to be received, shows if bitcoind node was already working when first msg
    ///arrives (zmq_seq==0)
    SeqStart { bitcoind_already_working: bool },
}

impl TryFrom<Msg> for MempoolSequence {
    type Error = ZMQSeqListenerError;

    fn try_from(msg: Vec<Vec<u8>>) -> Result<Self, Self::Error> {
        //Safe to do next three unwraps
        if msg.len() < 3 {
            return Err(ZMQSeqListenerError::MsgError("msg.len() < 3".to_string()));
        };
        let topic = str::from_utf8(msg.get(0).unwrap())?;
        let body = msg.get(1).unwrap();
        let zmqseq = msg.get(2).unwrap();

        if topic.ne("sequence") {
            return Err(ZMQSeqListenerError::TopicError(topic.to_string()));
        }
        let zmq_seq = parse_zmq_seq_num(zmqseq)?;
        let char = *body.get(32).ok_or(ZMQSeqListenerError::MsgError(
            "Not enough size, expected [u8;33]".to_string(),
        ))?;
        match char {
            // "C"
            67 => Ok(MempoolSequence::BlockConnection {
                block_hash: hex::encode(&body[..32]),
                zmq_seq,
            }),
            // "D"
            68 => Ok(MempoolSequence::BlockDisconnection {
                block_hash: hex::encode(&body[..32]),
                zmq_seq,
            }),
            // "R"
            82 => Ok(MempoolSequence::TxRemoved {
                txid: hex::encode(&body[..32]),
                mp_seq_num: parse_mp_seq_num(body)?,
                zmq_seq,
            }),
            // "A"
            65 => Ok(MempoolSequence::TxAdded {
                txid: hex::encode(&body[..32]),
                mp_seq_num: parse_mp_seq_num(body)?,
                zmq_seq,
            }),
            ch => Err(ZMQSeqListenerError::CharCodeError(ch.to_string())),
        }
    }
}

fn parse_mp_seq_num(value: &[u8]) -> Result<u64, ZMQSeqListenerError> {
    let ch: [u8; 8] = match value[33..41].try_into() {
        Ok(val) => Ok(val),
        Err(_) => Err(ZMQSeqListenerError::MsgError(
            "Not enough size, expected [u8;41]".to_string(),
        )),
    }?;
    Ok(u64::from_le_bytes(ch))
}

fn parse_zmq_seq_num(value: &[u8]) -> Result<u32, ZMQSeqListenerError> {
    let ch: [u8; 4] = match value[..4].try_into() {
        Ok(val) => Ok(val),
        Err(_) => Err(ZMQSeqListenerError::MsgError(
            "Not enough size, expected [u8;4]".to_string(),
        )),
    }?;
    Ok(u32::from_le_bytes(ch))
}

/// A bitcoincore ZMQ subscriber for receiving zmqpubsequence as defined [here](https://github.com/bitcoin/bitcoin/blob/master/doc/zmq.md)
///
/// This is a syncronous library that launches a listening thread which uses a start/stop interface
/// and receive data via a channel receiver.
///
/// [`MempoolSequence::SeqStart`] is the first data received containing whether bitcoind node has
/// started while we were listening, or the node was already up and running. This is important because in the fist case, the channel receiver will broadcast every block and
/// tx during node syncronization with the network. As this is normally not intended, use the
/// [`check::NodeChecker::wait_till_node_ok`] method available when using `check_node` cargo
/// feature.
///
/// After [`MempoolSequence::SeqStart`] a sequence of [`MempoolSequence::BlockConnection`],
/// [`MempoolSequence::BlockDisconnection`], [`MempoolSequence::TxAdded`] and
/// [`MempoolSequence::TxRemoved`] follows.
///
/// If any error happens, a [`MempoolSequence::SeqError`] will be sent.
///
/// Note that this Listener is aware of message interruptions, that is, if a zmq_seq number
/// is skipped, a [`MempoolSequence::SeqError`] with a [`ZMQSeqListenerError::InvalidSeqNumber`] is
/// sent.
///
/// Example:
/// ```
///    let zmqseqlistener = ZmqSeqListener::start(&Url::from_str("tcp://localhost:29000")?)?;
///    loop{
///        let kk = zmqseqlistener.rx.recv()?;
///        println!("{:?}", kk);
///    }
///
/// ```
pub struct ZmqSeqListener {
    pub rx: Receiver<MempoolSequence>,
    pub stop: Arc<AtomicBool>,
    pub thread: JoinHandle<()>,
}

impl ZmqSeqListener {
    ///Starts listening ZMQ messages given the `zmq_address`.
    pub fn start(zmq_address: &Url) -> Result<Self, ZMQSeqListenerError> {
        let context = zmq::Context::new();
        let subscriber = context.socket(zmq::SUB)?;
        subscriber.connect(zmq_address.as_str())?;
        subscriber.set_subscribe(b"sequence")?;
        let stop_th = Arc::new(AtomicBool::new(false));
        let stop = stop_th.clone();
        let (tx, rx) = channel();
        //Use a barrier, Zmq is "slow joiner"
        let barrier = Arc::new(Barrier::new(2));
        let barrierc = barrier.clone();
        let thread = thread::spawn(move || {
            let mut is_starting = true;
            let mut last_zmq_seq = 0;
            while !stop_th.load(Ordering::SeqCst) {
                let mpsq = match receive_mpsq(&subscriber) {
                    Ok(mpsq) => mpsq,
                    Err(e) => MempoolSequence::SeqError { error: e },
                };
                if is_starting {
                    barrier.wait();
                    tx.send(MempoolSequence::SeqStart {
                        bitcoind_already_working: check_bitcoind_already_working(&mpsq),
                    })
                    .unwrap();
                    is_starting = false;
                } else {
                    let zmq_seq = zmq_seq_from(&mpsq);
                    if zmq_seq.is_some() {
                        if zmq_seq.unwrap() != last_zmq_seq + 1 {
                            tx.send(MempoolSequence::SeqError {
                                error: ZMQSeqListenerError::InvalidSeqNumber(
                                    last_zmq_seq + 1,
                                    zmq_seq.unwrap(),
                                ),
                            })
                            .unwrap();
                        }
                    }
                }
                last_zmq_seq = zmq_seq_from(&mpsq).unwrap_or(last_zmq_seq);
                tx.send(mpsq).unwrap();
            }
        });
        barrierc.wait();
        Ok(ZmqSeqListener { rx, stop, thread })
    }
}

fn check_bitcoind_already_working(mpsq: &MempoolSequence) -> bool {
    matches!(
        mpsq,
        MempoolSequence::BlockConnection { zmq_seq, .. }
            | MempoolSequence::BlockDisconnection { zmq_seq, .. }
            | MempoolSequence::TxRemoved { zmq_seq, .. }
            | MempoolSequence::TxAdded { zmq_seq, .. }
            if *zmq_seq != 0
    )
}

fn zmq_seq_from(mpsq: &MempoolSequence) -> Option<u32> {
    match mpsq {
        MempoolSequence::BlockConnection { zmq_seq, .. }
        | MempoolSequence::BlockDisconnection { zmq_seq, .. }
        | MempoolSequence::TxRemoved { zmq_seq, .. }
        | MempoolSequence::TxAdded { zmq_seq, .. } => Some(*zmq_seq),
        _ => None,
    }
}

fn receive_mpsq(subscriber: &zmq::Socket) -> Result<MempoolSequence, ZMQSeqListenerError> {
    let res = {
        let msg = subscriber.recv_multipart(0)?;
        let mpsq = MempoolSequence::try_from(msg)?;
        Ok(mpsq)
    };
    res
}
