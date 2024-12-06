use std::thread;
use crate::test::test_bench::{my_generic_fragment_forward, test_butterfly_flood, test_double_chain_flood, test_flood, test_star_flood};
use crate::initializer::initialize;

mod sim_app;
mod sim_control;
mod initializer;
mod skylink_drone;
mod test;

fn main() {
    println!("Hello, world!");

    // Put test = false if you want to use the interface.
    // Put test = true if you want to use the test_bench.
    let test = true;
    if test {
        //Comment functions we aren't testing

        // my_generic_fragment_forward();
        // generic_chain_fragment_ack();
        // generic_fragment_drop();
        // generic_chain_fragment_drop();
        // test_generic();
        // test_double_chain_flood();
        // test_star_flood();
        test_butterfly_flood()
    } else {
        let (sim_contr, handles) = initialize("input.toml");
        sim_app::run_simulation_gui(sim_contr);

        for handle in handles.into_iter() {
            handle.join().unwrap();
        }
    }

}
