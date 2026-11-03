//! Limits for untrusted input (gate 1, G8/G14): the sizes of masks and images derive from
//! document EMUs, the kernel radius from effect parameters, the path points and the fill work
//! from custom geometry. Every size is computed in `u64` and checked before allocating.
//!
//! The defaults depend on the target ([`Limits::DEFAULT`]): on wasm32 the memory of a phone is
//! far below 4 GiB, so the mask and image caps are lower there and a limit error comes before an
//! allocation failure. The wasm32 `max_mask_pixels` is **below** a padded text glow (up to about
//! 48 M pixels): on wasm32 such a glow returns `Err(Limit(MaskPixels))` where the TS paints, a
//! listed difference (G8, A2) until the phone budget of G12 is met.

/// Limits applied by the allocating functions of this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// pixels of a mask or of a padded plane (`w * h`), also what `Mask::new`, `morph`,
    /// `shift_mask`, `warp_mask` and `shade_bevel` check
    pub max_mask_pixels: u64,
    /// the radius `max(1, ceil(3 * sigma))` of a Gaussian blur kernel
    pub max_kernel_radius: u32,
    /// points one `flatten_path` call produces
    pub max_path_points: u64,
    /// rows times edges of one `fill_polygons` call (the scan is O(rows x 4 x edges))
    pub max_fill_work: u64,
    /// formulas per list (`av`, `gd`) `custom_guides` evaluates; the rest is dropped, as Slides'
    /// `slice(0, 2000)` does
    pub max_custom_guides: u32,
    /// pixels of an image `adjust_sharpen` works on (it allocates 24 to 28 bytes per pixel)
    pub max_image_pixels: u64,
}

impl Limits {
    /// The native defaults: 2^26 mask pixels (above every padded layer the TS callers build, at
    /// most about 48 M), 4,096 kernel radius, 2^22 path points, 2^30 fill work, 2,000 guides
    /// per list and 2^28 image pixels.
    ///
    /// `max_fill_work` is provisional: the module plan leaves its value to the security
    /// specialist, who sets it from measured times above what the TS callers produce (a real
    /// outline has at most about 10^4 edges over at most about 5 * 10^3 rows: 5 * 10^7).
    pub const NATIVE: Limits = Limits {
        max_mask_pixels: 1 << 26,
        max_kernel_radius: 4_096,
        max_path_points: 1 << 22,
        max_fill_work: 1 << 30,
        max_custom_guides: 2_000,
        max_image_pixels: 1 << 28,
    };

    /// The wasm32 defaults: 2^25 mask pixels and 2^25 image pixels; the rest as
    /// [`Limits::NATIVE`].
    pub const WASM32: Limits = Limits { max_mask_pixels: 1 << 25, max_image_pixels: 1 << 25, ..Limits::NATIVE };

    /// The defaults of the target: [`Limits::WASM32`] with 32-bit pointers, [`Limits::NATIVE`]
    /// elsewhere.
    #[cfg(target_pointer_width = "32")]
    pub const DEFAULT: Limits = Limits::WASM32;

    /// The defaults of the target: [`Limits::WASM32`] with 32-bit pointers, [`Limits::NATIVE`]
    /// elsewhere.
    #[cfg(not(target_pointer_width = "32"))]
    pub const DEFAULT: Limits = Limits::NATIVE;
}

impl Default for Limits {
    fn default() -> Self {
        Limits::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_and_wasm32_defaults() {
        assert_eq!(Limits::NATIVE.max_mask_pixels, 64 << 20);
        assert_eq!(Limits::NATIVE.max_kernel_radius, 4_096);
        assert_eq!(Limits::NATIVE.max_path_points, 4 << 20);
        assert_eq!(Limits::NATIVE.max_fill_work, 1 << 30);
        assert_eq!(Limits::NATIVE.max_custom_guides, 2_000);
        assert_eq!(Limits::NATIVE.max_image_pixels, 256 << 20);
        assert_eq!(Limits::WASM32.max_mask_pixels, 32 << 20);
        assert_eq!(Limits::WASM32.max_image_pixels, 32 << 20);
        assert_eq!(Limits::WASM32.max_kernel_radius, Limits::NATIVE.max_kernel_radius);
        assert_eq!(Limits::WASM32.max_path_points, Limits::NATIVE.max_path_points);
        assert_eq!(Limits::WASM32.max_fill_work, Limits::NATIVE.max_fill_work);
        assert_eq!(Limits::WASM32.max_custom_guides, Limits::NATIVE.max_custom_guides);
        assert_eq!(Limits::DEFAULT, if cfg!(target_pointer_width = "32") { Limits::WASM32 } else { Limits::NATIVE });
        assert_eq!(Limits::default(), Limits::DEFAULT);
    }
}
