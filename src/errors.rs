use std::str::Utf8Error;

use thiserror::Error;

///All possible errors from a ZMQ message
#[derive(Error, Debug, PartialEq, Eq, Clone)]
pub enum ZMQSeqListenerError {
    ///Error from ZMQ library
    #[error("Error from ZMQ library")]
    ZMQError(#[from] zmq::Error),
    ///Error parsing message, normally due to Utf8Error
    #[error("Error parsing msg")]
    ParseError(#[from] Utf8Error),
    ///Error in msg size or structure
    #[error("Message error: {0}")]
    MsgError(String),
    ///Invalid char code in message.
    #[error("Invalid char code: {0} in mempoolseq message")]
    CharCodeError(String),
    ///Invalid topic in message
    #[error("Topic error, topic should be \"secuence\", but its: {0}")]
    TopicError(String),
    ///Invalid sequence number
    #[error("Invalid ZMQ secuence number, should be {0}, but it is {1}")]
    InvalidSeqNumber(u32, u32),
}
