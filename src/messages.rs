extern crate crypto;
extern crate reed_solomon;

use self::reed_solomon::{Encoder, Decoder};
use self::crypto::sha2::Sha256;
use self::crypto::digest::Digest;

use shares::{split_data, unite_data, DataShare};

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

/// Calculate a hash of T, to obtain messageId.
fn calc_message_id(t: &[u8]) -> Result<Vec<u8>,()> {
    let mut hasher = Sha256::new();
    let output_len = hasher.output_bytes();
    if output_len < MESSAGE_ID_LEN {
        // The hash output be at least as long as message_id.
        return Err(());
    }
    let mut sha256_hash = vec![0; output_len];
    hasher.input(&t);
    hasher.result(&mut sha256_hash);

    // Take only the part we care about:
    sha256_hash.truncate(MESSAGE_ID_LEN);
    Ok(sha256_hash)
}

/// Split a message m into a few Fragmentos messages, to be sent to the destination.
/// Could fail if message is too large.
/// Returns a list of Fragmentos messages (As vectors) to be sent to the remote side.
fn split_message(m: &[u8], nonce: &[u8; NONCE_LEN], max_datagram: usize) 
        -> Result<Vec<Vec<u8>>,()> {

    if m.len() > max_message(max_datagram)? {
        return Err(())
    }

    let len_without_padding = MESSAGE_ID_LEN + 1 + m.len();
    let space_in_msg = max_datagram - (MESSAGE_ID_LEN + 1 + 1 + ECC_LEN);
    let b = (len_without_padding + space_in_msg - 1) / space_in_msg;

    let padding_count = (b - (len_without_padding % b)) % b;

    // Construct T:
    let mut t = Vec::new();

    t.extend_from_slice(nonce);     // nonce8
    t.push(padding_count as u8);    // paddingCount
    t.extend_from_slice(m);         // M
    // Push zeroes for padding:
    for _ in 0 .. padding_count {
        t.push(0);
    }

    let message_id = calc_message_id(&t)?;
    let data_shares = split_data(m, b as u8)?;
    let enc =  Encoder::new(ECC_LEN);

    Ok(data_shares
        .into_iter()
        .enumerate()
        .map(|(i, data_share)| {
            let mut fmessage = Vec::new();
            fmessage.extend_from_slice(&message_id);
            fmessage.push(b as u8);
            fmessage.push(i as u8);
            fmessage.extend_from_slice(&data_share.data);
            let encoded = enc.encode(&fmessage);
            fmessage.extend_from_slice(encoded.ecc());
            fmessage
        }).collect::<Vec<Vec<u8>>>()
    )
}

/// Reconstruct a message given a list of data shares.
fn unite_message(message_id: &[u8; MESSAGE_ID_LEN], data_shares: &[DataShare]) 
        -> Result<Vec<u8>,()> {

    let b = data_shares.len();
    if b == 0 {
        return Err(());
    }

    let t = unite_data(data_shares)?;

    // Make sure that the provided message_id matches the calculated message_id:
    let c_message_id = calc_message_id(&t)?;
    if message_id != &c_message_id[..] {
        return Err(());
    }

    // let nonce = &t[0..NONCE_LEN];
    let padding_count = t[NONCE_LEN] as usize;
    let m = &t[NONCE_LEN + 1 .. t.len() - padding_count];

    Ok(m.to_vec())
}

/// Read a fragmentos message and possibly correct it using the given error correction code.
/// If the message is valid, doesn't change the message and returns true.
/// If correction occurred and succeeded, return true. Otherwise, return false.
fn correct_frag_message(frag_message: &mut [u8]) -> bool {
    let dec = Decoder::new(ECC_LEN);
    if !dec.is_corrupted(&frag_message) {
        // Message is not corrupted, we have nothing to do.
        return true;
    }

    // We are here if the message is corrupted. We try to fix it:
    match dec.correct(frag_message, None) {
        Ok(_) => true,      // message corrected successfuly
        Err(_) => false,    // could not correct message
    }
}
