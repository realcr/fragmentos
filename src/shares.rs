
use reed_solomon_erasure;
use reed_solomon_erasure::{ReedSolomon, option_shards_into_shards};


#[derive(Debug, Ord, PartialOrd, Eq, PartialEq)]
pub struct DataShare {
    pub input: u8, 
    pub data: Vec<u8>,
}


#[derive(Debug)]
pub enum SplitDataError {
    NumBlocksIsZero,
    NumBlocksTooLarge,
    ReedSolomonInitFailed(reed_solomon_erasure::Error),
    ReedSolomonEncodeFailed(reed_solomon_erasure::Error),
}


/// Split data to 2b - 1 blocks, where every b blocks can reconstruct the original data.
/// (2*b - 1) must be smaller or equal to 256.
pub fn split_data(data: &[u8], b: u8) -> Result<Vec<DataShare>,SplitDataError> {
    let num_blocks = b as usize;

    if num_blocks == 0 {
        return Err(SplitDataError::NumBlocksIsZero);
    }

    if 2*num_blocks - 1 > 256 {
        return Err(SplitDataError::NumBlocksTooLarge);
    }

    if num_blocks == 1 {
        return Ok(vec![DataShare {
            input: 0,
            data: data.to_vec(),
        }])
    }

    // Special case of just one block. We don't need to use reed-solomon encoder.
    // Note that we will get an error if we try to use the 
    // reed solomon encoder with amount of parity shards = 0
    let reed_solomon = match ReedSolomon::new(num_blocks, num_blocks - 1) {
        Ok(reed_solomon) => reed_solomon,
        Err(e) => return Err(SplitDataError::ReedSolomonInitFailed(e)),
    };

    let block_size = (data.len() + (num_blocks - 1)) / num_blocks;

    // Add zero padding in case block_size is not a divisor of data.len():
    let padding_len = num_blocks * block_size - data.len();
    debug_assert!(padding_len < block_size);

    let mut cdata = data.to_vec();
    for _ in 0 .. padding_len {
        cdata.push(0);
    }

    let mut shards: Vec<Box<[u8]>> = Vec::new();

    for i in 0 .. num_blocks {
        let cur_shard = cdata[i*block_size .. (i+1)*block_size].to_vec();
        debug_assert!(cur_shard.len() == block_size);
        shards.push(cur_shard.into_boxed_slice());
    }

    // Add extra num_blocks - 1 empty shards to be used for encoding:
    for _ in 0 .. num_blocks - 1 {
        shards.push(vec![0u8; block_size].into_boxed_slice());
    }

    match reed_solomon.encode_shards(&mut shards) {
        Ok(()) => {},
        Err(e) => return Err(SplitDataError::ReedSolomonEncodeFailed(e)),
    };

    Ok(shards
        .into_iter()
        .enumerate()
        .map(|(i, shard)| {
            DataShare {
                input: i as u8,
                data: shard.to_vec(),
            }
        }).collect::<Vec<DataShare>>())
}

#[derive(Debug)]
pub enum UniteDataError {
    NumBlocksIsZero,
    NumBlocksTooLarge,
    ReedSolomonInitFailed(reed_solomon_erasure::Error),
    ReedSolomonDecodeFailed(reed_solomon_erasure::Error),
}

/// Reconstruct original data using given b data shares
/// Reconstructed data might contain trailing zero padding bytes.
pub fn unite_data(data_shares: &[DataShare]) -> Result<Vec<u8>,UniteDataError> {

    let num_blocks = data_shares.len();

    if num_blocks == 0 {
        return Err(UniteDataError::NumBlocksIsZero);
    }

    // Limit due to the amount of elements in the field.
    if (2 * num_blocks - 1) > 256 {
        return Err(UniteDataError::NumBlocksTooLarge);
    }

    // Special case of just one block. We don't need to use reed-solomon decoder.
    if num_blocks == 1 {
        return Ok(data_shares[0].data.clone());
    }

    let reed_solomon = match ReedSolomon::new(num_blocks, num_blocks - 1) {
        Ok(reed_solomon) => reed_solomon,
        Err(e) => return Err(UniteDataError::ReedSolomonInitFailed(e)),
    };

    // Convert data_shares into shards format:
    let mut option_shards: Vec<Option<Box<[u8]>>> = vec![None; 2*num_blocks - 1];
    for data_share in data_shares {
        let cloned_share_data = data_share.data.clone();
        option_shards[data_share.input as usize] = Some(cloned_share_data.into_boxed_slice());
    }

    match reed_solomon.reconstruct_shards(&mut option_shards) {
        Ok(()) => {},
        Err(e) => return Err(UniteDataError::ReedSolomonDecodeFailed(e)),
    };


    let shards = option_shards_into_shards(option_shards);

    // Reconstruct original data (Possibly with trailing zero padding):
    let mut res_data = Vec::new();
    for i in 0 .. num_blocks {
        res_data.extend_from_slice(&shards[i]);
    }

    Ok(res_data)

}

#[cfg(test)]
mod tests {
    extern crate rand;
    extern crate test;
    use super::*;
    use self::rand::{StdRng, Rng};
    use self::test::Bencher;

    /*
    #[test]
    fn split_and_unite_block() {
        let orig_block: &[u8] = &[1,2,3,4,5,6,7];
        let shares = split_block(&orig_block).unwrap();
        let new_block = unite_block(&shares[0 .. orig_block.len()]).unwrap();
        assert_eq!(orig_block, &new_block[..]);
    }
    */

    #[test]
    fn split_unite_data() {
        let my_data = &[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20];

        for b in 1 .. 5_usize {
            let data_shares = split_data(my_data, b as u8).unwrap();

            let mut new_data = unite_data(&data_shares[0 .. b]).unwrap();
            assert_eq!(new_data.len(), 
                       b * ((my_data.len() + b - 1) / b));

            // Truncate resulting data, as it might contain some trailing padding zeroes.
            new_data.truncate(my_data.len());
            assert_eq!(my_data, &new_data[..]);
        }

    }

    #[bench]
    fn bench_unite_data(bencher: &mut Bencher) {
        let seed: &[_] = &[1,2,3,4,5];
        let mut rng: StdRng = rand::SeedableRng::from_seed(seed);
        let mut my_data = vec![0; 2500];
        rng.fill_bytes(&mut my_data);

        let b: usize = 5;
        let data_shares = split_data(&my_data, b as u8).unwrap();

        bencher.iter(|| unite_data(&data_shares[0 .. b]).unwrap());
    }


}
