extern crate gf256;

use self::gf256::Gf256;

#[derive(Debug)]
struct Share {
    input : u8, // Input to the polynomial
    output: u8, // Output from the polynomial (at input)
}

/// Split a block of length b bytes into 2b - 1 shares.
fn split_block(block: &[u8]) -> Option<Vec<Share>> {
    let b = block.len();

    // We treat the block as a polynomial P of degree b - 1 with b coefficients.
    // (Element block[0] is the most significat coefficient).
    // We then calculate P(0), P(1), ... P(2b - 1)
    // Every b points should be enough to reconstruct the original polynomial.
    
    // We are not able to split to more than 256 shares because we are using a field with 256
    // elements (Gf256).
    if (2*b - 1) > 256 {
        return None
    }
    
    Some((0 .. (2*b - 1) as u8)
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
fn unite_block(shares: &[Share]) -> Option<Vec<u8>> {
    let b = shares.len();

    if (b == 0) || ((2*b - 1) > 256) {
        // If b == 0 we have no shares to use.
        // If 2*b - 1 > 256 there must be some shares that correspond to the same input, as there
        // are only 256 elements in the field. We abort.
        return None;
    }

    if b == 1 {
        // Only one share means that we don't need to do any interpolation. 
        // We just provide the share output back as a vector of one byte.
        return Some(vec![shares[0].output])
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
                return None
            }
            c = c / diff;
        }


        // Multiply the numerator polynomial by c_i, and add to the final total polynomial result:
        for j in 0 .. b {
            res_poly[j] = res_poly[j] + (cur_numerator[j] * c);
        }
    }

    Some(res_poly
         .into_iter()
         .map(|g_x| g_x.to_byte())
         .collect::<Vec<u8>>()
    )
}

struct DataShare {
    input: u8, 
    data: Vec<u8>,
}


/// Split data to 2b - 1 blocks, where every b can reconstruct the original data.
/// (2*b - 1) must be smaller or equal to 256.
fn split_data(data: &[u8], b: u8) -> Result<Vec<DataShare>,()> {
    if 2*b - 1 > 256 {
        return Err(());
    }
    // 2*b - 1 = num_shares
    let block_size: usize = b as usize;
    
    // We divide the data into blocks.
    // If not exactly divisible by block size, we add extra 0 padding bytes.
    let num_blocks = (data.len() + (block_size - 1)) / block_size;

    let data_shares = Vec::new();
    for i in 0 .. (2*b - 1) {
        data_shares.push(DataShare {
            input: i,
            data: Vec::new(),
        });
    }

    let process_block = |block| -> Result<(),()> {
        let shares = match split_block(block) {
            None => return Err(()),
            Some(shares) => shares,
        };

        for share in &shares {
            assert!(data_shares[share.input as usize].input == share.input);
            data_shares[share.input as usize].data.push(share.output);
        }
        Ok(())
    };


    for i in 0 .. (data.len() / block_size) {
        process_block(&data[i*block_size .. (i+1)*block_size])?;
    }

    // Deal with possible left bytes (Because block_size does not 
    // divide data.len()):
    
    let left_bytes = data.len() - block_size * (data.len() / block_size);
    if left_bytes > 0 {
        let last_block = Vec::<u8>::new();
        last_block.extend_from_slice(
            &data[
                block_size * (data.len() / block_size) ..
                data.len()
            ]
        );
        assert_eq!(last_block.len(), left_bytes);

        // Add padding bytes:
        for _ in 0 .. (block_size - left_bytes) {
            last_block.push(0);
        }
        assert_eq!(last_block.len(), block_size);
        process_block(&last_block)?;
    }

    Ok(data_shares)
}

/// Reconstruct original data using given b data shares
fn unite_data(shares: Vec<DataShare>, length: usize) -> Vec<u8> {
    let block_size = shares.len();

    let res_data = Vec::<u8>::new();

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_and_unite_block() {
        let orig_block: &[u8] = &[1,2,3,4,5,6,7];
        let shares = split_block(&orig_block).unwrap();
        let new_block = unite_block(&shares[0 .. orig_block.len()]).unwrap();
        assert_eq!(orig_block, &new_block[..]);
    }

}
