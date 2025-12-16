
pub use smart_leds::{SmartLedsWrite, RGB8};
pub use heapless::Vec;


/// Number of WS2812 LEDs in the LED strip
pub const LED_NUMBER: usize = 1;
/// Max value for a color component: R, G or B
pub const AMPLITUDE: u16 = 8;
/// Store all the LED color values for the LED strip
pub type ColorFrame = [RGB8; LED_NUMBER];

/// Make pastel color not so pastel by decreasing two of three RGB components
pub fn no_pastel(color: RGB8) -> RGB8 {

    let mut res = color;

    if color.r == max(color.r, color.g, color.b) {
        if color.r % 2 == 0 {
            res.g /= 3;
        }
        else {
            res.b /= 3;
        } 
    } else if color.g == max(color.r, color.g, color.b) {
        if color.g % 2 == 0 {
            res.r /= 3;
        }
        else {
            res.b /= 3;
        } 
    } else {
        if color.b % 2 == 0 {
            res.r /= 3;
        }
        else {
            res.g /= 3;
        } 
    }

    res
}

/// Color pattern made of single points
pub fn single_point(color: RGB8) -> Vec<RGB8, 1> {
    let mut res = Vec::new();
    res.push(color).ok();
    res
}

const LEN: usize = (AMPLITUDE*6) as usize;

/// Color pattern with brightness changing as triangle wave /\
pub fn triangle_wave(color: RGB8) -> Vec<RGB8, LEN> {
    let mut res = Vec::new();

    let len = max(color.r, color.g, color.b);
    
    for _ in 1..=(len*2) {

        res.push(RGB8::default()).unwrap();
    }
    for x in 1..=len {

        let value = mul_on_ratio(color, x, len);
        res.push(value).unwrap();
    }
    for x in (2..=len).rev() {

        let value = mul_on_ratio(color, x, len);
        res.push(value).unwrap();
    }
    for _ in 1..=(len*2) {

        res.push(RGB8::default()).unwrap();
    }

    res
}


fn max(one: u8, two: u8, three: u8) -> u8 {
    max2(one, max2(two, three))
}

fn max2(one: u8, two: u8) -> u8 {
    if one > two {
        one
    } else {
        two
    }
}

fn mul_on_ratio(color: RGB8, numerator: u8, denominator: u8) -> RGB8 {

    RGB8 {

        r: (color.r as u16 * numerator as u16 / denominator as u16) as u8,
        g: (color.g as u16 * numerator as u16 / denominator as u16) as u8,
        b: (color.b as u16 * numerator as u16 / denominator as u16) as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::{assert_eq, assert_ne};
    

    #[test]
    fn correct_ratio() {

        let res = mul_on_ratio(
            RGB8 {
                r: 20,
                g: 10,
                b: 5
            },
            1,
            5);

        assert_eq!(
            RGB8 {
                r: 4,
                g: 2,
                b: 1
            },
            res);
    }
}
