use indexmap::IndexSet;
pub trait Cell {
    /// Returns the total number of alive neighbours for a given cell
    fn neighbours(&self, xy: u32, image_size: u32) -> Option<usize>;
}

impl Cell for IndexSet<u32> {
    fn neighbours(&self, xy: u32, image_size: u32) -> Option<usize> {
        let mut live_neighbours = 0;

        // DONT NEED BECAUSE WANT TO PASS **EVERY** COORDINATE IN SLICE (world) RATHER THAN JUST LIVE CELLS
        // pattern match instead of unwrap to avoid panicking if requested value is not in IndexSet
        // let xy = match self.get_index(index) {
        //     Some(coordinate) => coordinate,
        //     None => return None,
        // };
        let coord_bits = (image_size as f32).log2() as u32;
        let x = xy >> coord_bits;
        let y = xy & ((1 << coord_bits) - 1);
        
        // Skip if coordinates are out of bounds
        if x >= image_size || y >= image_size {
            return Some(0);
        }

        let i32_image= image_size as i32;
        // Check each neighbor only if it would be within bounds
        for dx in -1..=1i32 {
            for dy in -1..=1i32 {
                // Skip the cell itself
                if dx == 0 && dy == 0 {
                    continue;
                }
                
                // Calculate potential neighbor coordinates
                let new_x = (x as i32 + dx + i32_image) % i32_image;
                let new_y = (y as i32 + dy + i32_image) %i32_image;
                
                // Check bounds
                if new_x >= 0 && new_x < image_size as i32 && 
                   new_y >= 0 && new_y < image_size as i32 {
                    // Combine coordinates back into single value
                    let neighbor = ((new_x as u32) << coord_bits) | (new_y as u32);
                    if self.contains(&neighbor) {
                        live_neighbours += 1;
                    }
                }
            }
        }


        Some(live_neighbours)
    }
}