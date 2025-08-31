mod app;
mod buffer;
mod editor;
mod keymap;
mod ui;

fn main() -> std::io::Result<()> {
    app::run()
}

