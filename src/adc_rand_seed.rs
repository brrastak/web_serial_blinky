//! This module is used to generate a truly random seed for random number generator
//! application should read values of ADC, assigned to a not connected pin
//! the adc_seed function takes as an argument a heapless vector that contains those values
//! and composes LSB of each value into a seed value

pub use heapless::Vec;


/// Compose LSBs of each value in vector into a seed value
pub fn adc_seed<const LEN: usize>(vec: Vec<u16, LEN>) -> u64 {

    assert!(vec.len() > 0);
    assert!(vec.len() <= u64::BITS as usize);

    let mut res = 0u64;
    for value in vec {

        res <<= 1;
        if (value & 0x01) != 0 {
            res += 1;
        }
    }

    res
}


#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::{assert_eq, assert_ne};
    

    #[test]
    fn get_1111_0000() {

        let expectations: Vec<u8, 8> = [
            0x0000_0001,    // LSB = 1
            0x0000_00FF,    // LSB = 1
            0x0000_FFFF,    // LSB = 1
            0xFFFF_FFFF,    // LSB = 1
            0x0000_0000,    // LSB = 0
            0x0000_00FE,    // LSB = 0
            0x0000_FFFE,    // LSB = 0
            0xFFFF_FFFE,    // LSB = 0
        ];

        assert_eq!(0b1111_0000, adc_seed(expectations));
    }
}
