fn main() {
    if let Err(err) = gwbasic::run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
