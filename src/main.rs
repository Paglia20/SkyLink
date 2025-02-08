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
pub const CLIENT_GIO: bool = true;
pub const ALL_CHAT: bool = false;
pub const ALL_CONTENT: bool = true;
pub const DEBUG_MODE : bool = false;
pub const NO_SERVER_MODE: bool = false; //provvisoria finchè non ci sono i server
pub const AUTOMATIC_FLOOD: bool = false; //fast as fuck boi


fn main() {
    // println!("Hello, world!");
    //change switch to change the run
    let switch = Switch::SimDaniel;

    match switch {
        Switch::SimDaniel => {
            if let Some((sim_contr, handles)) = initialize("inputs/input_star_with_pdr.toml") {
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

            // test_generic_fragment_forward();
            // test_generic_drop();
            // test_generic_nack();
            // test_flood();
            // test_double_chain_flood();
            // test_star_flood();
            // test_butterfly_flood();
            // test_tree_flood();
            // test_drone_commands();
            // test_busy_network();

        }
    }
}

/* we will have to change this Switch and change the client spawned if gio or sam */
enum Switch {
    Test,
    SimDaniel,
}
