extern crate rand;
extern crate fragmentos;

use fragmentos::{split_data, unite_data};
use self::rand::{StdRng, Rng};

const DATA_SIZE: usize = 1 << 13;
const DATAGRAM_SIZE: usize = 500;
const NUM_ITERS: usize = 0x100;

fn main() {
    let seed: &[_] = &[1,2,3,4,5];
    let mut rng: StdRng = rand::SeedableRng::from_seed(seed);
    let mut my_data = vec![0; DATA_SIZE];
    rng.fill_bytes(&mut my_data);

    let b: usize = (DATA_SIZE / DATAGRAM_SIZE) + 1;
    let data_shares = split_data(&my_data, b as u8).unwrap();

    // We use the xorer to make sure this will not be optimized out:
    let mut xorer: u8 = 0;
    for _ in 0 .. NUM_ITERS {
        let my_result = unite_data(&data_shares[0 .. b]).unwrap();
        xorer ^= my_result[1];
    }
    println!("xorer = {}", xorer);
}
