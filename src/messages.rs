extern crate crypto;
extern crate reed_solomon;

use self::reed_solomon::Encoder;
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

`T := nonce8 || paddingCount || M || padding`
*/

// Length of messageId (First 8 bytes of sha256 of the underlying T data):
const MESSAGE_ID_LEN: usize = 8;
// Length in bytes of the Reed Solomon error correcting code:
const ECC_LEN: usize = 8;
// Length in bytes of nonce in the beginning of the underlying T data:
const NONCE_LEN: usize = 8;


/// Calculate max possible message for Fragmentos, given the maximum datagram allowed on the
/// underlying protocol.
fn max_message(max_datagram: usize) -> Result<usize,()> {
    let fields_len = MESSAGE_ID_LEN + 1 + 1 + ECC_LEN;
    if max_datagram <= fields_len {
        Err(())
    } else {
        Ok((128 * (max_datagram - fields_len)) - (NONCE_LEN + 1))
    }
}

/// Split a message m into a few Fragmentos messages, to be sent to the destination.
/// Could fail if message is too large.
fn split_message(m: &[u8], nonce: &[u8; NONCE_LEN], max_datagram: usize) -> Result<Vec<Vec<u8>>,()> {
    if m.len() > max_message(max_datagram)? {
        return Err(())
    }

    let len_without_padding = MESSAGE_ID_LEN + 1 + m.len();
    let space_in_msg = max_datagram - (MESSAGE_ID_LEN + 1 + 1 + ECC_LEN);
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
    let output_len = hasher.output_bytes();
    if output_len < MESSAGE_ID_LEN {
        // The result of the hash hash to at least as long as message_id.
        return Err(());
    }
    let mut sha256_hash = vec![0; output_len];
    hasher.input(&t);
    hasher.result(&mut sha256_hash);
    let message_id = &sha256_hash[0..MESSAGE_ID_LEN];

    let data_shares = split_data(m, b as u8)?;

    let enc =  Encoder::new(ECC_LEN);

    Ok(data_shares
        .into_iter()
        .enumerate()
        .map(|(i, data_share)| {
            let mut fmessage = Vec::new();
            fmessage.extend_from_slice(message_id);
            fmessage.push(b as u8);
            fmessage.push(i as u8);
            fmessage.extend_from_slice(&data_share.data);
            let encoded = enc.encode(&fmessage);
            fmessage.extend_from_slice(encoded.ecc());
            fmessage
        }).collect::<Vec<Vec<u8>>>()
    )
}
