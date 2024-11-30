use crate::initializer::initialize;

mod my_drone;
mod sim_app;
mod sim_control;
mod initializer;

fn main() {

    println!("Hello, world!");
    //sim_app::run_simulation_gui();
    let mut handles = initialize("input.toml").1;

    while let Some(handle) = handles.get_mut().pop() {
        handle.join().unwrap();
    }
}