use vek::Rgb;

pub fn srgb_to_linear(color: Rgb<f32>) -> Rgb<f32> {
    color.map(|channel| {
        let channel = channel.max(0.0);
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    })
}

pub fn linear_to_srgb(color: Rgb<f32>) -> Rgb<f32> {
    color.map(|channel| {
        let channel = channel.max(0.0);
        if channel <= 0.0031308 {
            channel * 12.92
        } else {
            1.055 * channel.powf(1.0 / 2.4) - 0.055
        }
    })
}

/// A light color is an sRGB hue times a linear intensity, and the brightest channel holds the
/// intensity. The hue is decoded and the intensity stays, so a grey light keeps its value and
/// a lamp intensity keeps its meaning.
pub fn light_to_linear(color: Rgb<f32>) -> Rgb<f32> {
    let intensity = color.reduce_partial_max();
    if intensity <= 0.0 {
        return Rgb::zero();
    }
    srgb_to_linear(color / intensity) * intensity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_srgb_round_trip() {
        for value in 0..=255 {
            let channel = value as f32 / 255.0;
            let back = linear_to_srgb(srgb_to_linear(Rgb::broadcast(channel)));
            assert!((back.r - channel).abs() < 1.0e-5);
        }
    }

    #[test]
    fn test_light_to_linear() {
        assert_eq!(light_to_linear(Rgb::broadcast(0.3)), Rgb::broadcast(0.3));
        assert_eq!(light_to_linear(Rgb::zero()), Rgb::zero());
        let fire = light_to_linear(Rgb::new(6.0, 2.4, 0.6));
        assert!((fire.r - 6.0).abs() < 1.0e-5);
        assert!((fire.g - 6.0 * srgb_to_linear(Rgb::broadcast(0.4)).g).abs() < 1.0e-5);
    }
}
