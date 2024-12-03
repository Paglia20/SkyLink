use crate::initializer::initialize;

mod my_drone;
mod sim_app;
mod sim_control;
mod initializer;
mod testbench;

fn main() {

    println!("Hello, world!");
    //sim_app::run_simulation_gui();
    let handles = initialize("input.toml");
    for handle in handles.into_iter() {
        handle.join().unwrap();
    }
}