pub trait Coordinates {
    /// returns `(coordinate_length, offset)`
    ///
    /// `coordinate_length` is the length of the combined `xy` coordinate.
    /// - For 16x16 this will be 8 bits.
    /// - For 64x64 this will be 12 bits.
    /// - For 512x512 this will be 18 bits.
    ///
    /// `offset` is the remaining number of bits in the `u32` type that are unused by the coordinate
    /// - For 16x16 this will be 24 bits.
    /// - For 64x64 this will be 20 bits.
    /// - For 512x512 this will be 14 bits.
    fn calc_coord_len_and_offset(image_size: u32) -> (u32, u32);

    /// generates mask of left aligned 1's where there are `coordinate_length` number of 1's
    fn generate_mask(coordinate_length: u32) -> u32;
    /// returns the limit the decoder should wait for the bit count to reach before continuing to next chunk
    fn limit(coordinate_length: u32) -> usize;
}