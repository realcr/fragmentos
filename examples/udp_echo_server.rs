#![feature(conservative_impl_trait)]

extern crate futures;
extern crate tokio_core;

extern crate fragmentos;

use std::net::SocketAddr;
use std::{env, io};
use std::time::Instant;

use futures::Future;

use tokio_core::net::UdpSocket;
use tokio_core::reactor::Core;

use fragmentos::frag_msg_receiver::FragMsgReceiver;
use fragmentos::frag_msg_sender::FragMsgSender;

const MAX_DGRAM_LEN: usize = 512;

fn get_cur_instant() -> Instant {
    Instant::now()
}

struct SocketHolder {
    opt_udp_socket: Option<UdpSocket>,
}

/*
impl SocketHolder {
    fn recv_dgram<B>(&mut self, recv_buff: B) -> impl Future<Item=(B, usize, SocketAddr), Error=io::Error>
    where
        B: AsMut<[u8]>,
    {
        let fut_recv_dgram = match self.opt_udp_socket.take() {
            None => panic!("Socket is already in use!"),
            Some(udp_socket) => udp_socket.recv_dgram(recv_buff)
        };

        fut_recv_dgram.and_then(|(udp_socket, recv_buff, size, socket_address)| {
            self.opt_udp_socket = Some(udp_socket);
            Ok((recv_buff, size, socket_address))
        })
    }
}

*/

fn main() {
    /*
    let addr = env::args().nth(1).unwrap_or("127.0.0.1:8080".to_string());
    let addr = addr.parse::<SocketAddr>().unwrap();

    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let socket = UdpSocket::bind(&addr, &handle).unwrap();

    let recv_dgram = |buff| {
        socket.recv_dgram(buff)
            .and_then(|(_udp_socket, buff, size, socket_addr)| {
                Ok((buff, size, socket_addr))
            })
    };

    let fmr = FragMsgReceiver::new(get_cur_instant, recv_dgram, MAX_DGRAM_LEN);
    */
}
