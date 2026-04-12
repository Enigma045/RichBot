use model::RequestTracker;

mod model;
mod brain;

fn main() {
    let tracker = model::RequestTracker::new();
    println!("Current mode: {}", tracker.task_mode);
}
