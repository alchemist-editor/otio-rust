//! Upstream's float formatting, digit for digit.
//!
//! OTIO writes JSON through RapidJSON, whose `dtoa` is Grisu2. Grisu2 is fast
//! and always round-trips, but it does not always find the shortest digits:
//! it writes `0.0054767390000000004` where the shortest form is
//! `0.005476739`. Both read back as the same number, but a file written here
//! should be byte for byte the file upstream writes, so this is RapidJSON's
//! `internal/dtoa.h` and `internal/diyfp.h` ported as they are written.

/// A number as a 64-bit significand and a binary exponent: `f × 2^e`.
#[derive(Clone, Copy)]
struct DiyFp {
    f: u64,
    e: i32,
}

const SIGNIFICAND_SIZE: i32 = 52;
const EXPONENT_BIAS: i32 = 0x3FF + SIGNIFICAND_SIZE;
const MIN_EXPONENT: i32 = -EXPONENT_BIAS;
const EXPONENT_MASK: u64 = 0x7FF0_0000_0000_0000;
const SIGNIFICAND_MASK: u64 = 0x000F_FFFF_FFFF_FFFF;
const HIDDEN_BIT: u64 = 0x0010_0000_0000_0000;

impl DiyFp {
    fn from_f64(value: f64) -> Self {
        let bits = value.to_bits();
        let biased = ((bits & EXPONENT_MASK) >> SIGNIFICAND_SIZE) as i32;
        let significand = bits & SIGNIFICAND_MASK;
        if biased == 0 {
            Self {
                f: significand,
                e: MIN_EXPONENT + 1,
            }
        } else {
            Self {
                f: significand + HIDDEN_BIT,
                e: biased - EXPONENT_BIAS,
            }
        }
    }

    /// The product, its low half rounded into the high.
    fn times(self, other: Self) -> Self {
        let product = u128::from(self.f) * u128::from(other.f);
        #[expect(clippy::cast_possible_truncation, reason = "the halves of a u128")]
        let (mut high, low) = ((product >> 64) as u64, product as u64);
        if low & (1 << 63) != 0 {
            high += 1;
        }
        Self {
            f: high,
            e: self.e + other.e + 64,
        }
    }

    fn normalize(self) -> Self {
        #[expect(clippy::cast_possible_wrap, reason = "at most 64")]
        let shift = self.f.leading_zeros() as i32;
        Self {
            f: self.f << shift,
            e: self.e - shift,
        }
    }

    fn normalize_boundary(self) -> Self {
        let mut result = self;
        while result.f & (HIDDEN_BIT << 1) == 0 {
            result.f <<= 1;
            result.e -= 1;
        }
        let shift = 64 - SIGNIFICAND_SIZE - 2;
        result.f <<= shift;
        result.e -= shift;
        result
    }

    /// The boundaries halfway to the neighbouring doubles, below and above.
    fn normalized_boundaries(self) -> (Self, Self) {
        let plus = Self {
            f: (self.f << 1) + 1,
            e: self.e - 1,
        }
        .normalize_boundary();
        let mut minus = if self.f == HIDDEN_BIT {
            Self {
                f: (self.f << 2) - 1,
                e: self.e - 2,
            }
        } else {
            Self {
                f: (self.f << 1) - 1,
                e: self.e - 1,
            }
        };
        minus.f <<= minus.e - plus.e;
        minus.e = plus.e;
        (minus, plus)
    }
}

/// `10^k` for `k = -348, -340, …, 340`, as normalized significands and
/// binary exponents, each correctly rounded.
const CACHED_POWERS: [(u64, i32); 87] = [
    (0xfa8fd5a0_081c0288, -1220),
    (0xbaaee17f_a23ebf76, -1193),
    (0x8b16fb20_3055ac76, -1166),
    (0xcf42894a_5dce35ea, -1140),
    (0x9a6bb0aa_55653b2d, -1113),
    (0xe61acf03_3d1a45df, -1087),
    (0xab70fe17_c79ac6ca, -1060),
    (0xff77b1fc_bebcdc4f, -1034),
    (0xbe5691ef_416bd60c, -1007),
    (0x8dd01fad_907ffc3c, -980),
    (0xd3515c28_31559a83, -954),
    (0x9d71ac8f_ada6c9b5, -927),
    (0xea9c2277_23ee8bcb, -901),
    (0xaecc4991_4078536d, -874),
    (0x823c1279_5db6ce57, -847),
    (0xc2109436_4dfb5637, -821),
    (0x9096ea6f_3848984f, -794),
    (0xd77485cb_25823ac7, -768),
    (0xa086cfcd_97bf97f4, -741),
    (0xef340a98_172aace5, -715),
    (0xb23867fb_2a35b28e, -688),
    (0x84c8d4df_d2c63f3b, -661),
    (0xc5dd4427_1ad3cdba, -635),
    (0x936b9fce_bb25c996, -608),
    (0xdbac6c24_7d62a584, -582),
    (0xa3ab6658_0d5fdaf6, -555),
    (0xf3e2f893_dec3f126, -529),
    (0xb5b5ada8_aaff80b8, -502),
    (0x87625f05_6c7c4a8b, -475),
    (0xc9bcff60_34c13053, -449),
    (0x964e858c_91ba2655, -422),
    (0xdff97724_70297ebd, -396),
    (0xa6dfbd9f_b8e5b88f, -369),
    (0xf8a95fcf_88747d94, -343),
    (0xb9447093_8fa89bcf, -316),
    (0x8a08f0f8_bf0f156b, -289),
    (0xcdb02555_653131b6, -263),
    (0x993fe2c6_d07b7fac, -236),
    (0xe45c10c4_2a2b3b06, -210),
    (0xaa242499_697392d3, -183),
    (0xfd87b5f2_8300ca0e, -157),
    (0xbce50864_92111aeb, -130),
    (0x8cbccc09_6f5088cc, -103),
    (0xd1b71758_e219652c, -77),
    (0x9c400000_00000000, -50),
    (0xe8d4a510_00000000, -24),
    (0xad78ebc5_ac620000, 3),
    (0x813f3978_f8940984, 30),
    (0xc097ce7b_c90715b3, 56),
    (0x8f7e32ce_7bea5c70, 83),
    (0xd5d238a4_abe98068, 109),
    (0x9f4f2726_179a2245, 136),
    (0xed63a231_d4c4fb27, 162),
    (0xb0de6538_8cc8ada8, 189),
    (0x83c7088e_1aab65db, 216),
    (0xc45d1df9_42711d9a, 242),
    (0x924d692c_a61be758, 269),
    (0xda01ee64_1a708dea, 295),
    (0xa26da399_9aef774a, 322),
    (0xf209787b_b47d6b85, 348),
    (0xb454e4a1_79dd1877, 375),
    (0x865b8692_5b9bc5c2, 402),
    (0xc83553c5_c8965d3d, 428),
    (0x952ab45c_fa97a0b3, 455),
    (0xde469fbd_99a05fe3, 481),
    (0xa59bc234_db398c25, 508),
    (0xf6c69a72_a3989f5c, 534),
    (0xb7dcbf53_54e9bece, 561),
    (0x88fcf317_f22241e2, 588),
    (0xcc20ce9b_d35c78a5, 614),
    (0x98165af3_7b2153df, 641),
    (0xe2a0b5dc_971f303a, 667),
    (0xa8d9d153_5ce3b396, 694),
    (0xfb9b7cd9_a4a7443c, 720),
    (0xbb764c4c_a7a44410, 747),
    (0x8bab8eef_b6409c1a, 774),
    (0xd01fef10_a657842c, 800),
    (0x9b10a4e5_e9913129, 827),
    (0xe7109bfb_a19c0c9d, 853),
    (0xac2820d9_623bf429, 880),
    (0x80444b5e_7aa7cf85, 907),
    (0xbf21e440_03acdd2d, 933),
    (0x8e679c2f_5e44ff8f, 960),
    (0xd433179d_9c8cb841, 986),
    (0x9e19db92_b4e31ba9, 1013),
    (0xeb96bf6e_badf77d9, 1039),
    (0xaf87023b_9bf0ee6b, 1066),
];

/// The cached power that brings a number with binary exponent `e` into
/// Grisu's range, and the decimal exponent it scales by, negated.
fn cached_power(e: i32) -> (DiyFp, i32) {
    let dk = f64::from(-61 - e) * 0.301_029_995_663_981_14 + 347.0;
    #[expect(clippy::cast_possible_truncation, reason = "a small positive number")]
    let mut k = dk as i32;
    if dk - f64::from(k) > 0.0 {
        k += 1;
    }
    let index = (k >> 3) + 1;
    let decimal = -(-348 + (index << 3));
    #[expect(clippy::cast_sign_loss, reason = "positive by construction")]
    let (f, e) = CACHED_POWERS[index as usize];
    (DiyFp { f, e }, decimal)
}

const POW10: [u64; 20] = [
    1,
    10,
    100,
    1_000,
    10_000,
    100_000,
    1_000_000,
    10_000_000,
    100_000_000,
    1_000_000_000,
    10_000_000_000,
    100_000_000_000,
    1_000_000_000_000,
    10_000_000_000_000,
    100_000_000_000_000,
    1_000_000_000_000_000,
    10_000_000_000_000_000,
    100_000_000_000_000_000,
    1_000_000_000_000_000_000,
    10_000_000_000_000_000_000,
];

fn grisu_round(buffer: &mut [u8], delta: u64, mut rest: u64, ten_kappa: u64, wp_w: u64) {
    while rest < wp_w
        && delta - rest >= ten_kappa
        && (rest + ten_kappa < wp_w || wp_w - rest > rest + ten_kappa - wp_w)
    {
        if let Some(last) = buffer.last_mut() {
            *last -= 1;
        }
        rest += ten_kappa;
    }
}

fn count_decimal_digits(n: u32) -> u32 {
    match n {
        0..10 => 1,
        10..100 => 2,
        100..1_000 => 3,
        1_000..10_000 => 4,
        10_000..100_000 => 5,
        100_000..1_000_000 => 6,
        1_000_000..10_000_000 => 7,
        10_000_000..100_000_000 => 8,
        _ => 9,
    }
}

/// Writes the digits between the boundaries and adjusts `k` to match.
fn digit_gen(w: DiyFp, mp: DiyFp, mut delta: u64, buffer: &mut Vec<u8>, k: &mut i32) {
    let shift = -mp.e;
    let one = 1u64 << shift;
    let wp_w = mp.f.wrapping_sub(w.f);
    #[expect(clippy::cast_possible_truncation, reason = "the integral part fits")]
    let mut p1 = (mp.f >> shift) as u32;
    let mut p2 = mp.f & (one - 1);
    let mut kappa = count_decimal_digits(p1);

    while kappa > 0 {
        let divisor = 10u32.pow(kappa - 1);
        let digit = p1 / divisor;
        p1 %= divisor;
        if digit != 0 || !buffer.is_empty() {
            #[expect(clippy::cast_possible_truncation, reason = "a single digit")]
            buffer.push(b'0' + digit as u8);
        }
        kappa -= 1;
        let rest = (u64::from(p1) << shift) + p2;
        if rest <= delta {
            #[expect(clippy::cast_possible_wrap, reason = "at most 9")]
            {
                *k += kappa as i32;
            }
            grisu_round(buffer, delta, rest, POW10[kappa as usize] << shift, wp_w);
            return;
        }
    }

    let mut kappa: i32 = 0;
    loop {
        p2 = p2.wrapping_mul(10);
        delta = delta.wrapping_mul(10);
        #[expect(clippy::cast_possible_truncation, reason = "a single digit")]
        let digit = (p2 >> shift) as u8;
        if digit != 0 || !buffer.is_empty() {
            buffer.push(b'0' + digit);
        }
        p2 &= one - 1;
        kappa -= 1;
        if p2 < delta {
            *k += kappa;
            #[expect(clippy::cast_sign_loss, reason = "kappa is negative here")]
            let index = (-kappa) as usize;
            let scale = POW10.get(index).copied().unwrap_or(0);
            grisu_round(buffer, delta, p2, one, wp_w.wrapping_mul(scale));
            return;
        }
    }
}

/// The digits of a positive finite number and the power of ten they scale
/// by: `value ≈ digits × 10^k`.
fn grisu2(value: f64) -> (Vec<u8>, i32) {
    let v = DiyFp::from_f64(value);
    let (w_m, w_p) = v.normalized_boundaries();
    let (c_mk, mut k) = cached_power(w_p.e);
    let w = v.normalize().times(c_mk);
    let mut wp = w_p.times(c_mk);
    let mut wm = w_m.times(c_mk);
    wm.f += 1;
    wp.f -= 1;
    let mut buffer = Vec::with_capacity(20);
    digit_gen(w, wp, wp.f - wm.f, &mut buffer, &mut k);
    (buffer, k)
}

/// Lays the digits out as RapidJSON does: plainly while the decimal point
/// falls within `-6 < point <= 21`, else in exponential form.
fn prettify(digits: &[u8], k: i32, out: &mut String) {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "at most 17 digits"
    )]
    let length = digits.len() as i32;
    let kk = length + k;
    let text = |digits: &[u8]| digits.iter().map(|&d| char::from(d)).collect::<String>();
    if (0..).contains(&k) && kk <= 21 {
        out.push_str(&text(digits));
        for _ in length..kk {
            out.push('0');
        }
        out.push_str(".0");
    } else if 0 < kk && kk <= 21 {
        #[expect(clippy::cast_sign_loss, reason = "positive in this branch")]
        let (whole, fraction) = digits.split_at(kk as usize);
        out.push_str(&text(whole));
        out.push('.');
        out.push_str(&text(fraction));
    } else if -6 < kk && kk <= 0 {
        out.push_str("0.");
        for _ in kk..0 {
            out.push('0');
        }
        out.push_str(&text(digits));
    } else if length == 1 {
        out.push_str(&text(digits));
        out.push('e');
        out.push_str(&(kk - 1).to_string());
    } else {
        out.push_str(&text(&digits[..1]));
        out.push('.');
        out.push_str(&text(&digits[1..]));
        out.push('e');
        out.push_str(&(kk - 1).to_string());
    }
}

/// Formats a finite float as RapidJSON's `Writer::Double` does.
///
/// RapidJSON's `maxDecimalPlaces` is left at its default of 324, which
/// never truncates, so the branches that apply it are not ported.
pub(crate) fn dtoa(value: f64) -> String {
    let mut out = String::with_capacity(24);
    if value == 0.0 {
        if value.is_sign_negative() {
            out.push('-');
        }
        out.push_str("0.0");
        return out;
    }
    let value = if value < 0.0 {
        out.push('-');
        -value
    } else {
        value
    };
    let (digits, k) = grisu2(value);
    prettify(&digits, k, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::dtoa;

    #[test]
    fn writes_what_upstream_writes() {
        // Each checked against OpenTimelineIO's own writer.
        for (value, expected) in [
            // The shortest digits are 0.005476739; Grisu2 does not find them.
            (0.005_476_739, "0.0054767390000000004"),
            (0.1, "0.1"),
            (1e21, "1e21"),
            (1e-7, "1e-7"),
            (123_456_789.123, "123456789.123"),
            (5e-324, "5e-324"),
            (f64::MAX, "1.7976931348623157e308"),
            (2.0 / 3.0, "0.6666666666666666"),
            (1.0 / 3.0 * 1e-5, "0.0000033333333333333333"),
            (24.0, "24.0"),
            (-0.0, "-0.0"),
            (-1.5, "-1.5"),
        ] {
            assert_eq!(dtoa(value), expected, "{value:e}");
        }
    }
}
