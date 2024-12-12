use std::cell::RefCell;
use std::rc::Rc;
use crate::sim_daniel::*;
use crate::test::test_bench::*;
use crate::initializer::initialize;
use crate::sim_control::SimulationControl;

mod sim_sam;
mod sim_control;
mod initializer;
mod skylink_drone;
mod test;
mod sim_daniel;


fn main() {
    // println!("Hello, world!");
    //change switch to change the run
    let switch = Switch::SimDaniel;

    match switch {
        Switch::SimDaniel => {
            let (sim_contr, handles) = initialize("inputs/input_star.toml");
            let mut pass = Rc::new(RefCell::new(sim_contr));
            run_sim_dan(pass).expect("TODO: panic message");

            for handle in handles.into_iter() {
                handle.join().unwrap();
            }
        }
        Switch::SimSam => {
            let (sim_contr, handles) = initialize("inputs/input_generic_fragment_forward.toml");
            let mut pass = Rc::new(RefCell::new(sim_contr));
            pass.borrow_mut().crash_drone(2);
            sim_sam::run_simulation_gui(pass.clone());

            for handle in handles.into_iter() {
                handle.join().unwrap();
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



enum Switch {
    Test,
    SimDaniel,
    SimSam,
}

