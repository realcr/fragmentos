
/*
Fragmentos message:

- messageId         [8 bytes]
- b                 [1 byte]
- shareIndex        [1 byte]
- shareData         [variable amount of bytes]
- errorCorrection   [8 bytes]
*/


/// Calculate max possible message for Fragmentos, given the maximum datagram allowed on the
/// underlying protocol.
fn max_message(max_datagram: usize) -> Option<usize> {
    if max_datagram <= 18 {
        None
    } else {
        Some((128 * (max_datagram - 18)) - 9)
    }
}

/// Split a message m into a few fragmentos messages, to be sent to the destination.
/// Could fail if message is too large.
fn split_message(m: &[u8], nonce: &[u8; 8], max_datagram: usize) -> Result(Vec<Vec<u8>>,()) {
    if m.len() > max_message(max_datagram) {
        return Err(())
    }

    let mut t = Vec::new();

    // `T := nonce8 || paddingCount || M || padding`
    t.extend_from_slice(nonce);
    t.push(0);
    t.extend_from_slice(m);



}
