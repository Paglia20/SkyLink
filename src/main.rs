use std::cell::RefCell;
use std::rc::Rc;
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
    let test = false;
    if test {
        //Comment functions we aren't testing

        // test_generic_fragment_forward();
        // test_generic_drop();
        // test_generic_nack();
        // test_flood();
        test_double_chain_flood();
        // test_star_flood();
        // test_butterfly_flood();
        // test_tree_flood();
        // test_drone_commands();
        // test_busy_network();

        

    } else {
        let (sim_contr, handles) = initialize("inputs/input_generic_fragment_forward.toml");
        let mut pass = Rc::new(RefCell::new(sim_contr));
        pass.borrow_mut().crash_drone(2);
        sim_app::run_simulation_gui(pass.clone());



        for handle in handles.into_iter() {
            handle.join().unwrap();
        }
    }
}
