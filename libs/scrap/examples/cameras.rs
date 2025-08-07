extern crate scrap;

fn main() {
    let displays = scrap::camera::Cameras::all_info().unwrap();

    for (i, display) in displays.iter().enumerate() {
        println!("Display {} [{}x{}]", i + 1, display.width, display.height);
    }
}
