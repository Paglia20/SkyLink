use crate::initializer::initialize;
use crate::simulation_control::{sim_control, sim_daniel::*};

mod clients_gio;
mod initializer;
mod message;
mod network_edge;
mod routing;
mod server;
mod simulation_control;
mod test;
mod clients_sam;

//for testing
pub const CLIENT_GIO: bool = false; // If false run with sam's

pub const ALL_CHAT: bool = false;
pub const ALL_CONTENT: bool = false;

pub const DEBUG_MODE : bool = false;
pub const NO_SERVER_MODE: bool = false; // Temporary why servers aren't finished.
pub const AUTOMATIC_FLOOD: bool = false; //fast as fuck boi

fn main() {
    // Change 'switch' to change the run
    let switch = Switch::SimDaniel;
    // let switch = Switch::Test;

    // Topology
    let topology = "inputs/input.toml";
    let topology2 = "inputs/input_3_peat.toml";
    let topology3 = "inputs/input_flood.toml";


    match switch {
        Switch::SimDaniel => {
            println!("running {topology2}");
            if let Some((sim_contr, handles)) = initialize(topology2) {
                run_sim_dan(sim_contr).expect("Problem in running GUI");
                for handle in handles.into_iter() {
                    handle.join().unwrap();
                }
            }else {
               panic!("Input File Invalid")
            }
        }
        Switch::Test => {
            //Comment functions we aren't testing

            // test::test_bench::test_generic_fragment_forward();
            // test::test_bench::test_generic_drop();
            // test::test_bench::test_generic_nack();
            // test::test_bench::test_flood();
            // test::test_bench::test_double_chain_flood();
            // test::test_bench::test_star_flood();
            // test::test_bench::test_butterfly_flood();
            // test::test_bench::test_tree_flood();
            // test::test_bench::test_drone_commands();
            // test::test_bench::test_busy_network();
        }
    }
}

/* we will have to change this Switch and change the client spawned if gio or sam */
enum Switch {
    Test,
    SimDaniel,
}
