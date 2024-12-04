use crate::initializer::initialize;
use crate::test_bench::generic_fragment_forward;

mod my_drone;
mod sim_app;
mod sim_control;
mod initializer;
mod test_bench;

fn main() {
    println!("Hello, world!");
    //sim_app::run_simulation_gui();

    let test = true;
    if test {
        //Comment functions we aren't testing
        generic_fragment_forward();
    } else {
        let handles = initialize("input.toml");
        for handle in handles.into_iter() {
            handle.join().unwrap();
        }
    }
}