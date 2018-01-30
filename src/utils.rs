use std::io;
use std::net::SocketAddr;
use tokio_core::net::{UdpCodec};

pub struct DgramCodec;

/// A basic UDP codec. Turns every received message
/// into a tuple of the received message and the address 
/// of sender or recipient.
impl UdpCodec for DgramCodec {
    type In = (Vec<u8>, SocketAddr);
    type Out = (Vec<u8>, SocketAddr);

    fn decode(&mut self, src: &SocketAddr, buf: &[u8]) -> io::Result<Self::In> {
        Ok((buf.to_vec(), src.clone()))
    }
    
    fn encode(&mut self, msg: Self::Out, buf: &mut Vec<u8>) -> SocketAddr {
        let (dgram, address) = msg;
        buf.extend_from_slice(&dgram);
        address
    }

}
