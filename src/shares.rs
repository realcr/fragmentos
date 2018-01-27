extern crate gf256;

// use self::gf256::Gf256;

use reed_solomon_erasure;
use reed_solomon_erasure::{ReedSolomon, option_shards_into_shards};

/*
#[derive(Debug)]
struct Share {
    input : u8, // Input to the polynomial
    output: u8, // Output from the polynomial (at input)
}

/// Split a block of length b bytes into 2b - 1 shares.
fn split_block(block: &[u8]) -> Result<Vec<Share>,()> {
    let b = block.len();

    // If block is empty, return error.
    if b == 0 {
        return Err(());
    }

    // We treat the block as a polynomial P of degree b - 1 with b coefficients.
    // (Element block[0] is the most significant coefficient).
    // We then calculate P(0), P(1), ... P(2b - 1)
    // Every b points should be enough to reconstruct the original polynomial.
    
    // We are not able to split to more than 256 shares because we are using a field with 256
    // elements (Gf256).
    if (2*b - 1) > 256 {
        return Err(());
    }
    
    Ok((0 .. (2*b - 1) as u8)
        .map(|x| {
            let gf_x = Gf256::from_byte(x);
            let mut gf_sum = Gf256::zero();

            for i in 0 .. b {
                gf_sum = gf_sum * gf_x;
                gf_sum = gf_sum + Gf256::from_byte(block[i]);
            }
            Share{ 
                input: x, 
                output: gf_sum.to_byte(),
            }
        }).collect::<Vec<Share>>())
}

/// Unite a block given b shares.
fn unite_block(shares: &[Share]) -> Result<Vec<u8>,()> {
    let b = shares.len();

    if (b == 0) || ((2*b - 1) > 256) {
        // If b == 0 we have no shares to use.
        // If 2*b - 1 > 256 there must be some shares that correspond to the same input, as there
        // are only 256 elements in the field. We abort.
        return Err(());
    }

    if b == 1 {
        // Only one share means that we don't need to do any interpolation. 
        // We just provide the share output back as a vector of one byte.
        return Ok(vec![shares[0].output])
    }

    // Perform Lagrange interpolation to find out the coefficients of the original polynomial.
    // We expect a polynomial of degree b - 1 (b coefficients).
    
    // Iterate over all monomials ( ...(x-x_j)... / ...(x_i-x_j)... ) * y_i
    let mut res_poly = vec![Gf256::zero(); b];

    for i in 0 .. b {
        let mut cur_numerator = vec![Gf256::zero(); b];
        cur_numerator[0] = Gf256::one();

        // Iterate over multiplicands (x-x_j) in monomial:
        for j in 0 .. b {
            if j == i {
                // In this monomial (x-x_i) will be missing:
                continue
            }

            // Perform multiplication of (x-x_j) with current numerator.
            for k in (1 .. (j + 1)).rev() {
                cur_numerator[k] = cur_numerator[k] -
                    Gf256::from_byte(shares[j].input) * cur_numerator[k-1];
            }
        }

        // Calculate c_i := y_i / ( ... (x_i-x_j) ... )
        let mut c = Gf256::from_byte(shares[i].output);
        for j in 0 .. b {
            if j == i {
                continue
            }
            let diff = Gf256::from_byte(shares[i].input) - Gf256::from_byte(shares[j].input);
            // We have two shares for the same input value. Aborting.
            if diff == Gf256::zero() {
                return Err(());
            }
            c = c / diff;
        }


        // Multiply the numerator polynomial by c_i, and add to the final total polynomial result:
        for j in 0 .. b {
            res_poly[j] = res_poly[j] + (cur_numerator[j] * c);
        }
    }

    Ok(res_poly
         .into_iter()
         .map(|g_x| g_x.to_byte())
         .collect::<Vec<u8>>()
    )
}
*/

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
        let mut my_data = vec![0; 1024];
        rng.fill_bytes(&mut my_data);

        let b: usize = 3;
        let data_shares = split_data(&my_data, b as u8).unwrap();

        bencher.iter(|| unite_data(&data_shares[0 .. b]).unwrap());
    }

    /*
    // This example is used in the fragmentos spec.
    #[test]
    fn test_example_split_message() {
        let my_data = b"\x12\x34\x56\x78\x90\xab\xcd\xef\x55";
        let b = 4;

        let data_shares = split_data(my_data, b).unwrap();
        for dshare in &data_shares {
            print!("dshare.data = ");
            for x in &dshare.data {
                print!("{:02x} ", x);
            }
            println!();
        }
        assert!(false);
    }
    */

}
