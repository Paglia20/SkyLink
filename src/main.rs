use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use crate::test::test_bench::*;
use crate::initializer::initialize;
use crate::simulation_control::*;
use crate::simulation_control::sim_daniel::run_sim_dan;

mod simulation_control;
mod initializer;
mod skylink_drone;
mod test;

fn main() {
    // println!("Hello, world!");
    //change switch to change the run
    let switch = Switch::SimDaniel;

    match switch {
        Switch::SimDaniel => {
            let (sim_contr, handles) = initialize("inputs/input_star.toml");

            run_sim_dan(sim_contr).expect("TODO: panic message");
            for handle in handles.into_iter() {
                handle.join().unwrap();
            }
        }
        Switch::SimSam => {
            let (sim_contr, handles) = initialize("inputs/input_generic_fragment_forward.toml");
           // sim_sam::run_simulation_gui(sim_contr.clone()).expect("TODO: panic message");
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
            test_log()
        }
    }
}



enum Switch {
    Test,
    SimDaniel,
    SimSam,
}

