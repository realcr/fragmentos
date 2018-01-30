use ring::digest::{digest, SHA512_256};

use shares::{split_data, unite_data, DataShare};

/*
Fragmentos message fragment:

- messageId         [8 bytes]
- b                 [1 byte]
- shareIndex        [1 byte]
- shareData         [variable amount of bytes]
- shortHash         [8 bytes]   (First 8 bytes of Sha512/256)

`T := nonce8 || paddingCount || M || padding`
*/

// Length of the short_hash function output.
const SHORT_HASH_LEN: usize = 8;
// Length of messageId (First 8 bytes of sha256 of the underlying T data):
pub const MESSAGE_ID_LEN: usize = SHORT_HASH_LEN;
// Length in bytes of the Reed Solomon error correcting code:
pub const ECC_LEN: usize = 8;
// Length in bytes of nonce in the beginning of the underlying T data:
pub const NONCE_LEN: usize = 8;
// Length of all message fields, excluding shareData:
const FIELDS_LEN: usize = MESSAGE_ID_LEN + 1 + 1 + ECC_LEN;


/// Calculate max possible message for Fragmentos, given the maximum datagram allowed on the
/// underlying protocol.
pub fn max_message(max_dgram_len: usize) -> Result<usize,()> {
    if max_dgram_len <= FIELDS_LEN {
        return Err(());
    }

    Ok((128 * (max_dgram_len - FIELDS_LEN)) - (NONCE_LEN + 1))
}

fn short_hash(input_data: &[u8]) -> [u8; SHORT_HASH_LEN] {
    let mut hash_output = [0x0; SHORT_HASH_LEN];
    let digest_res = digest(&SHA512_256, input_data);
    hash_output.copy_from_slice(&digest_res.as_ref()[0 .. MESSAGE_ID_LEN]);
    hash_output
}


/*
/// Calculate a hash of T, to obtain messageId.
pub fn calc_message_id(t: &[u8]) -> [u8; MESSAGE_ID_LEN] {
    short_hash(t)
}
*/

/// Split a message m into a few Fragmentos messages, to be sent to the destination.
/// Could fail if message is too large.
/// Returns a list of Fragmentos messages (As vectors) to be sent to the remote side.
pub fn split_message(m: &[u8], nonce: &[u8; NONCE_LEN], max_dgram_len: usize) 
        -> Result<Vec<Vec<u8>>,()> {

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

    let message_id = short_hash(&t);
    let data_shares = match split_data(&t, b as u8) {
        Ok(data_shares) => data_shares,
        // TODO: Fix error handling here:
        Err(_) => return Err(()),
    };

    Ok(data_shares
        .into_iter()
        .enumerate()
        .map(|(i, data_share)| {
            let mut fmessage = Vec::new();
            fmessage.extend_from_slice(&message_id);
            fmessage.push(b as u8);
            fmessage.push(i as u8);
            fmessage.extend_from_slice(&data_share.data);
            let frag_hash = short_hash(&fmessage);
            fmessage.extend_from_slice(&frag_hash);
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

    let t = match unite_data(data_shares) {
        Ok(data) => data,
        // TODO: Fix error handling here:
        Err(_) => return Err(()),
    };

    // Make sure that the provided message_id matches the calculated message_id:
    let c_message_id = short_hash(&t);
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
pub fn verify_frag_message(frag_message: &[u8]) -> bool {
    if frag_message.len() < SHORT_HASH_LEN {
        false
    } else {
        let hashed_content = &frag_message[ .. frag_message.len() - SHORT_HASH_LEN];
        let hash_output = short_hash(hashed_content);
        &hash_output == &frag_message[frag_message.len() - SHORT_HASH_LEN .. ]
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use rand;
    use rand::{StdRng, Rng};
    use test::Bencher;

    #[test]
    fn test_max_message() {
        assert!(max_message(0).is_err());
        assert!(max_message(MESSAGE_ID_LEN + ECC_LEN + 1).is_err());
        assert!(max_message(512).unwrap() > 512);
    }

    #[test]
    fn test_calc_message_id() {
        short_hash(b"Dummy T message");
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
    fn test_verify_frag_message() {
        let orig_message = b"This is some message to be split";
        let mut frags = split_message(orig_message, 
                                  b"nonce123", 22).unwrap();

        // frags[0] should be valid:
        assert!(verify_frag_message(&frags[0]));

        // Make some changes to fragment 0, so that verification will fail:
        frags[0][5] = 0x41;
        frags[0][6] = 0x32;
        frags[0][7] = 0xfe;
        frags[0][10] = 0x29;
        assert!(!verify_frag_message(&frags[0]));
    }

    #[bench]
    fn bench_unite_message(bencher: &mut Bencher) {
        let seed: &[_] = &[1,2,3,4,5];
        let mut rng: StdRng = rand::SeedableRng::from_seed(seed);
        let mut orig_message = vec![0; 4096];
        rng.fill_bytes(&mut orig_message);

        let frags = split_message(&orig_message, 
                                  b"nonce123", 200).unwrap();
        let frag_len = frags[0].len();
        assert!(frags.len() > 1);

        let message_id = array_ref![&frags[0],0,MESSAGE_ID_LEN];
        let b = frags[0][MESSAGE_ID_LEN];

        let data_shares = &(0 .. b).map(|i| DataShare {
            input: i, 
            data: (&frags[i as usize][MESSAGE_ID_LEN + 1 + 1 .. frag_len - ECC_LEN]).to_vec()
        }).collect::<Vec<DataShare>>();

        bencher.iter(|| unite_message(message_id, &data_shares[0 .. b as usize]).unwrap());
    }
}
