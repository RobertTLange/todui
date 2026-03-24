fn main() {
    if let Err(error) = todui::cli::run(std::env::args_os()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
