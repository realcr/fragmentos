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
pub const MESSAGE_ID_LEN: usize = 8;
// Length in bytes of the Reed Solomon error correcting code:
pub const ECC_LEN: usize = 8;
// Length in bytes of nonce in the beginning of the underlying T data:
pub const NONCE_LEN: usize = 8;
// Length of all message fields, excluding shareData:
const FIELDS_LEN: usize = MESSAGE_ID_LEN + 1 + 1 + ECC_LEN;

pub fn max_supported_dgram_len() -> usize {
    255 - FIELDS_LEN
}

/// Calculate max possible message for Fragmentos, given the maximum datagram allowed on the
/// underlying protocol.
pub fn max_message(max_dgram_len: usize) -> Result<usize,()> {
    if max_dgram_len <= FIELDS_LEN {
        return Err(());
    }

    Ok((128 * (max_dgram_len - FIELDS_LEN)) - (NONCE_LEN + 1))
}

/// Calculate a hash of T, to obtain messageId.
pub fn calc_message_id(t: &[u8]) -> Result<Vec<u8>,()> {
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
pub fn split_message(m: &[u8], nonce: &[u8; NONCE_LEN], max_dgram_len: usize) 
        -> Result<Vec<Vec<u8>>,()> {

    if max_dgram_len > max_supported_dgram_len() {
        return Err(());
    }

    if m.len() > max_message(max_dgram_len)? {
        return Err(());
    }

    let len_without_padding = MESSAGE_ID_LEN + 1 + m.len();
    let space_in_msg = max_dgram_len - (MESSAGE_ID_LEN + 1 + 1 + ECC_LEN);
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
    let data_shares = split_data(&t, b as u8)?;
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
pub fn unite_message(message_id: &[u8; MESSAGE_ID_LEN], data_shares: &[DataShare]) 
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
pub fn correct_frag_message(frag_message: &[u8]) -> Option<Vec<u8>> {
    let dec = Decoder::new(ECC_LEN);
    if !dec.is_corrupted(&frag_message) {
        // Message is not corrupted, we have nothing to do.
        return Some(frag_message.to_vec())
    }

    // We are here if the message is corrupted. We try to fix it:
    // TODO: Remove the clone here in the next version of reed-solomon.
    // frag_message doesn't need to be mutable.
    match dec.correct(&mut frag_message.to_vec(), None) {
        Ok(recovered) => {
            // message corrected successfuly
            Some((*recovered).to_vec())
        },      
        Err(_) => None,    // could not correct message
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_message() {
        assert!(max_message(0).is_err());
        assert!(max_message(MESSAGE_ID_LEN + ECC_LEN + 1).is_err());
        assert!(max_message(512).unwrap() > 512);
    }

    #[test]
    fn test_calc_message_id() {
        calc_message_id(b"Dummy T message").unwrap();
    }

    #[test]
    fn test_split_unite_message() {
        let orig_message = b"This is some message to be split";
        let frags = split_message(orig_message, 
                                  b"nonce123", 22).unwrap();
        assert!(frags.len() > 1);

        let message_id = array_ref![&frags[0],0,MESSAGE_ID_LEN];
        let b = frags[0][MESSAGE_ID_LEN];

        // Make sure that all the frags have exactly the same size:
        let frag_len = frags[0].len();
        for frag in &frags {
            assert_eq!(frag.len(), frag_len);
        }

        let data_shares = &(0 .. b).map(|i| DataShare {
            input: i, 
            data: (&frags[i as usize][MESSAGE_ID_LEN + 1 + 1 .. frag_len - ECC_LEN]).to_vec()
        }).collect::<Vec<DataShare>>();

        let new_message = unite_message(message_id, &data_shares[0..b as usize]).unwrap();
        assert_eq!(orig_message, &new_message[..]);
    }

    #[test]
    fn test_correct_frag_message() {
        let orig_message = b"This is some message to be split";
        let mut frags = split_message(orig_message, 
                                  b"nonce123", 22).unwrap();

        let orig_frag0 = frags[0].to_vec();

        // Make at most ECC_LEN/2 changes to fragment 0:
        frags[0][5] = 0x41;
        frags[0][6] = 0x32;
        frags[0][7] = 0xfe;
        frags[0][10] = 0x29;

        let corrected = correct_frag_message(&mut frags[0]).unwrap();
        assert_eq!(corrected, orig_frag0);
    }
}
