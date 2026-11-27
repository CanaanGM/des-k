// The paint tests: the dim layer leaves the promised share.
use super::*;

// Spec 0026 AC-18: the layer's alpha is round(dim × 255 / 100), so each channel ends near
// c × (100 - dim) / 100: 40 leaves white at 60%, and 0 lays nothing over.
#[test]
fn dim_leaves_the_promised_share() {
    assert_eq!((dim_alpha(0), dim_alpha(40), dim_alpha(80)), (0, 102, 204));
    for dim in 0..=80u8 {
        let white = 255 * (255 - dim_alpha(dim) as u32) / 255;
        let promised = 255 * (100 - dim as u32) / 100;
        assert!(white.abs_diff(promised) <= 1, "dim {dim}: {white} vs {promised}");
    }
}
