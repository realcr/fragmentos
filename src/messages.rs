extern crate crypto;


use self::crypto::sha2::Sha256;
use self::crypto::digest::Digest;

use shares::{split_data, unite_data};
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
fn max_message(max_datagram: usize) -> Result<usize,()> {
    if max_datagram <= 18 {
        Err(())
    } else {
        Ok((128 * (max_datagram - 18)) - 9)
    }
}

/// Split a message m into a few fragmentos messages, to be sent to the destination.
/// Could fail if message is too large.
fn split_message(m: &[u8], nonce: &[u8; 8], max_datagram: usize) -> Result<Vec<Vec<u8>>,()> {
    if m.len() > max_message(max_datagram)? {
        return Err(())
    }

    // `T := nonce8 || paddingCount || M || padding`

    let len_without_padding = 8 + 1 + m.len();
    let space_in_msg = max_datagram - (8 + 1 + 1 + 8);
    let b = (len_without_padding + space_in_msg - 1) / space_in_msg;

    let padding_count = (b - (len_without_padding % b)) % b;

    let mut t = Vec::new();

    t.extend_from_slice(nonce);     // nonce8
    t.push(padding_count as u8);    // paddingCount
    t.extend_from_slice(m);         // M
    // Push zeroes for padding:
    for _ in 0 .. padding_count {
        t.push(0);
    }

    // Calculate sha256 of T:
    let mut hasher = Sha256::new();
    let mut sha256_hash = [0; 32];
    hasher.input(&t);
    hasher.result(&mut sha256_hash);

    let messageId = &sha256_hash[0..8];

    let data_shares = split_data(m, b as u8)?;

    data_shares
        .into_iter()
        .enumerate()
        .map(|(i, data_share)| {
            let mut fmessage = Vec::new();
            fmessage.extend_from_slice(messageId);
            fmessage.push(b as u8);
            fmessage.push(i as u8);
            fmessage.extend_from_slice(&data_share.data);

            // TODO: Add the error correcting code here:


        }).collect::<Vec<Vec<u8>>>()

    // pub fn split_data(data: &[u8], b: u8) -> Result<Vec<DataShare>,()> {



}
