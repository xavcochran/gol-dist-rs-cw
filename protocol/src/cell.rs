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

        let neighbour_positions = vec![
            (xy + 512) % image_size, // right
            (xy - 512) % image_size, // left
            (xy + 1) % image_size,   // up
            (xy - 1) % image_size,   // down
            (xy + 513) % image_size, // right up
            (xy + 511) % image_size, // right down
            (xy - 511) % image_size, // left down
            (xy - 513) % image_size, // left up
        ];

        for &pos in &neighbour_positions {
            if self.contains(&pos) {
                live_neighbours += 1;
            }
        }

        Some(live_neighbours)
    }
}
