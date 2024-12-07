use crate::test::test_bench::*;
use crate::initializer::initialize;

mod sim_app;
mod sim_control;
mod initializer;
mod skylink_drone;
mod test;

fn main() {
    // println!("Hello, world!");


    // Put this to true if you want to use tests
    // or to false if you want to use the Sim Contr application.
    let test = true;
    if test {
        //Comment functions we aren't testing

        // test_generic_fragment_forward();
        // test_generic_drop();
        // test_generic_nack();
        // test_flood();


        // generic_chain_fragment_ack();
        // generic_fragment_drop();
        // generic_chain_fragment_drop();
        // test_double_chain_flood();
        // test_star_flood()
        // test_butterfly_flood();
        // my_generic_fragment_forward();
        // test_tree_flood();

       // test_drone_commands();
        test_busy_network()

        

    } else {
        let (sim_contr, handles) = initialize("input_tree.toml");
        sim_app::run_simulation_gui(sim_contr);
        for handle in handles.into_iter() {
            handle.join().unwrap();
        }
    }
}
