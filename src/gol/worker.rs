use crate::gol::event::Event;
use crate::gol::Params;
use crate::util::cell::CellCoord;
use flume::Sender;
use tokio;

pub async fn worker(
    params: Params,
    v_diff: usize,
    y1: usize,
    y2: usize,
    turn: u32,
    world_thread: Vec<Vec<u8>>,
    events_thread: Sender<Event>,
    result_channel: Sender<Vec<Vec<u8>>>,
) {
    let mut slice_world = vec![vec![0; params.image_height]; (y2-y1)+1];
    // go through each row and column and calculate if the cell should be alive or dead
    for i in 0..params.image_width {
        for j in y1..=y2 {
            let mut alive_neighbors = 0;
            // check the 8 neighbors of the cell
            for x in 0..=2 {
                for y in 0..=2 {
                    if x == 1 && y == 1 {
                        continue;
                    }
                    // check if the neighbor is alive
                    let x_neighbour = ((i + x + params.image_width) - 1) % params.image_width;
                    let y_neighbour = ((j + y + params.image_height) - 1) % params.image_height;

                    if world_thread[y_neighbour][x_neighbour] == 255 {
                        alive_neighbors += 1;
                    }
                }
            }
            if world_thread[j][i] == 255 {
                if alive_neighbors < 2 || alive_neighbors > 3 {
                    set_cell(j, i, &world_thread, 0, &events_thread, turn);
                    slice_world[j - y1][i] = 0
                } else {
                    set_cell(j, i, &world_thread, 255, &events_thread, turn);
                    slice_world[j - y1][i] = 255
                }
            } else {
                if alive_neighbors == 3 {
                    set_cell(j, i, &world_thread, 255, &events_thread, turn);
                    slice_world[j - y1][i] = 255
                } else {
                    set_cell(j, i, &world_thread, 0, &events_thread, turn);
                    slice_world[j - y1][i] = 0
                }
            }
        }
    }
    match result_channel.send_async(slice_world).await {
        Ok(_) => {}
        Err(e) => println!("Error sending result: {:?}", e),
    }

}

fn set_cell(
    y: usize,
    x: usize,
    world: &Vec<Vec<u8>>,
    new_value: u8,
    events: &Sender<Event>,
    turn: u32,
) {
    if world[y][x] != new_value {
        //events <- CellFlipped{CompletedTurns: turn, Cell: util.Cell{X: x, Y: y}}
        match events.send(Event::CellFlipped {
            completed_turns: turn,
            cell: CellCoord { x, y },
        }) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("{}", e);
            }
        }
    }
}
