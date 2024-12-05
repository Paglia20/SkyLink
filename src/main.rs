use std::thread;
use crate::initializer::initialize;

mod sim_app;
mod sim_control;
mod initializer;
mod skylink_drone;
mod test;

fn main() {
    println!("Hello, world!");
    sim_app::run_simulation_gui();

    let test = true;
    if test {
        //Comment functions we aren't testing
        //my_generic_fragment_forward();
    } else {
        let (sim_contr, handles) = initialize("input.toml");
        sim_app::run_simulation_gui(sim_contr);

        for handle in handles.into_iter() {
            handle.join().unwrap();
        }
    }

    // generic_chain_fragment_ack();
    // generic_fragment_drop();
    // generic_chain_fragment_drop();

    // test_generic();
}
