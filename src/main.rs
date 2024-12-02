use crate::initializer::initialize;
use crate::testbench::create_sample_packet;

mod my_drone;
mod sim_app;
mod sim_control;
mod initializer;
mod testbench;

fn main() {

    println!("Hello, world!");
    //sim_app::run_simulation_gui();
    let mut handles = initialize("input.toml");


}